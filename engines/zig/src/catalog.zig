//! Component catalog (SPEC 5): the single source of truth for param names (validation of unknown
//! keys), defaults, parts and anchors, rendered as JSON and markdown for the LLM system prompt.

const std = @import("std");
const json = @import("json.zig");
const params = @import("params.zig");
const builders = @import("builders.zig");
const Allocator = std.mem.Allocator;

/// One row of a parameter table (see params.zig).
pub const Param = params.Row;

/// The component types, in catalog order. The tag name is the type's name in documents (`"type": "lumber"`).
/// Adding a type: add the tag here, an entry to `entries` (same position) and a `build` arm in builders.zig
/// (that switch is exhaustive, so a type without a builder does not compile).
pub const Type = enum { lumber, panel, cmu_wall, concrete, rebar, anchor_bolt, connector, truss, membrane, fill, insulation, flashing, joint, solid };

pub const Entry = struct {
    type: Type,
    summary: []const u8,
    params: []const Param,
    parts: []const u8,
    anchors: []const u8,
    draws: []const u8,
    /// One complete component object (valid on its own); `kerf schema <type>` prints it.
    example: []const u8 = "",

    pub fn name(self: *const Entry) []const u8 {
        return @tagName(self.type);
    }
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
    .{ .names = &.{"id"}, .def = "required", .desc = "unique slug [a-z][a-z0-9_]*; annotations and refs name it" },
    .{ .names = &.{"type"}, .def = "required", .desc = "one of the catalog types" },
    .{ .names = &.{"label"}, .def = "null", .desc = "short human label for summaries" },
    .{ .names = &.{"at"}, .def = "origin", .desc = "{anchor (default bottom_left), to: Ref | {ref, offset} | [x,y], offset: [dx,dy]}: translates the part so its anchor lands on `to`+offset. Point-list types (connector, membrane, fill, rebar path, concrete polygon, solid points) ignore `anchor`: literal [x,y] entries are relative to `to`+offset, Refs are absolute" },
    .{ .names = &.{"rotate"}, .def = "0", .desc = "degrees CCW about the placement point" },
    .{ .names = &.{"slope"}, .def = "null", .desc = "\"4:12\" rise:run, degrees, or \"@<component>\" (e.g. \"@truss\": inherit that member's slope, so pitch lives in ONE place; a truss with exterior right slopes the other way). Rotation about the placement point; adds to rotate" },
    .{ .names = &.{"mirror"}, .def = "false", .desc = "mirror the profile about its vertical centerline before placement" },
    .{ .names = &.{"z"}, .def = "null", .desc = "[z0,z1] absolute, or a number = centered there with the member's natural z thickness. Default: spans doc `run` (members along Z); members with a natural z thickness (lumber run x/y, truss, connector, path rebar, anchor_bolt) are centered on the first section view's `cut_z` (when it sets one, so they are cut, not hidden), else on the middle of `run`" },
    .{ .names = &.{"array"}, .def = "null", .desc = "{axis: x|y|z, count, spacing}: instance k offset by k*spacing; instances are id#0..id#n-1, refs to `id` mean instance 0, `id#k@anchor` addresses instance k" },
    .{ .names = &.{"embedded"}, .def = "type default", .desc = "drawn over cut solids and never occluded (rebar, anchor bolts)" },
    .{ .names = &.{"visible"}, .def = "true", .desc = "false hides the component from views and mesh" },
    .{ .names = &.{"shown"}, .def = "solid", .desc = "dashed = \"where occurs\" graphics: all edges in the hidden (dashed) pen, no hatch or cut mark, never hides anything, exempt from W_FLOATING / W_NEAR_MISS / W_OVERLAP; notes targeting it get \" (WHERE OCCURS)\" appended. Section views only (iso omits it)" },
    .{ .names = &.{"acknowledge"}, .def = "null", .desc = "[{code, reason}]: suppress that warning (e.g. W_UNTREATED_CONTACT) for this component; the reason prints as an I_ACK line in the summary and is logged. Errors cannot be acknowledged; I_* codes are accepted and ignored (`kerf schema acknowledge`)" },
};

pub const box_anchors = "top_left top_center top_right middle_left center middle_right bottom_left bottom_center bottom_right (of the profile box; rotate with the member)";

pub const entries: []const Entry = &.{
    .{
        .type = .lumber,
        .summary = "Sawn or engineered wood member (stud, plate, joist, beam, blocking, post). Standard view for a beam in a wall (flush beam, header): an ELEVATION along the wall, i.e. the beam seen lengthwise (run x, face wide) with the top and bottom plates interrupted where they butt it and king/jack studs (run y) at its ends. Draw the end-on section (run z, beam cut) only when the designer asks for it; either way, say in your report which reading you drew.",
        .params = params.rows(builders.LumberParams),
        .parts = "barrier (when set)",
        .anchors = "the 9 box anchors",
        .draws = "Section: run z cut => outline + wood X mark per ply (blocking: one diagonal); run x/y cut lengthwise => outline only; beyond => outline. Actual sizes: 2x 1.5 thick; 4x 3.5; 6x 5.5; depths x4 3.5, x6 5.5, x8 7.25, x10 9.25, x12 11.25 (6x8 7.5, 6x10 9.5, 6x12 11.5). Natural z thickness for run x/y.",
        .example = "{\"id\":\"stud\",\"type\":\"lumber\",\"size\":\"2x4\",\"run\":\"y\",\"face\":\"narrow\",\"length\":92.625,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,1.5]}}",
    },
    .{
        .type = .panel,
        .summary = "Sheathing, boards, gypsum, soffit, fascia/trim boards as a thin rectangle.",
        .params = params.rows(builders.PanelParams),
        .parts = "none",
        .anchors = "the 9 box anchors",
        .draws = "Cut rectangle with the material's hatch / cut mark (wood_board: one diagonal). Spans the document run along Z. Use `slope` for roof sheathing (rotates about the placement anchor).",
        .example = "{\"id\":\"roof_sheathing\",\"type\":\"panel\",\"material\":\"osb\",\"thickness\":0.4375,\"length\":66,\"run\":\"x\",\"slope\":\"4:12\",\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
    },
    .{
        .type = .cmu_wall,
        .summary = "Concrete masonry wall in section: face shells, grouted cells, mortar joints, bond beam.",
        .params = params.rows(builders.CmuParams),
        .parts = "course_1..course_n (1 = bottom), bond_beam, grout",
        .anchors = "9 box anchors + bond_beam_center, top_center, cell_center_top",
        .draws = "Box height = courses*8 - 0.375 (+0.375 with top_joint); the lowest course sits on the box bottom. Per course: two face shells (cut, cmu hatch), grouted cell (grout hatch) or an empty cell with the cross web as a beyond line, mortar joints as cut lines. 3D: 15.625\" units with 0.375\" head joints, running bond.",
        .example = "{\"id\":\"cmu\",\"type\":\"cmu_wall\",\"width\":8,\"courses\":3,\"bond_beam_courses\":1,\"at\":{\"anchor\":\"bottom_left\",\"to\":[0,0]}}",
    },
    .{
        .type = .concrete,
        .summary = "Cast-in-place concrete: rect/footing, free polygon, or a monolithic slab with turned-down edge.",
        .params = params.rows(builders.ConcreteParams),
        .parts = "footing (turndown zone), slab (slab zone), base (when base is set) for slab_edge; footing for shape footing",
        .anchors = "9 box anchors; slab_edge adds top_exterior (datum (0,0) at the exterior face), slab_top, footing_bottom_exterior, footing_bottom_interior, slab_bottom_interior, haunch_top, recess_bottom_exterior, recess_bottom_interior, recess_top_interior, base_bottom_interior and base_bottom_footing (with base)",
        .draws = "slab_edge local origin: exterior face at x=0, top of slab at y=0, footing bottom at y=-footing_depth. Cut region with the material hatch; parts are zones (rebar place.in, cover), not separate outlines.",
        .example = "{\"id\":\"slab\",\"type\":\"concrete\",\"shape\":\"slab_edge\",\"slab_thickness\":4,\"footing_width\":12,\"footing_depth\":18,\"at\":{\"anchor\":\"top_exterior\",\"to\":[0,0]}}",
    },
    .{
        .type = .rebar,
        .summary = "Reinforcing bar: a dot in section (along_z) or a line in the XY plane (path).",
        .params = &.{
            .{ .names = &.{"size"}, .def = "#4", .desc = "#3 .375, #4 .5, #5 .625, #6 .75, #7 .875, #8 1.0 (diameter in)" },
            .{ .names = &.{"mode"}, .def = "along_z", .desc = "along_z (continuous bar seen as a dot) or path (bar in the XY plane)" },
            .{ .names = &.{"place"}, .def = "null", .desc = "cover-based placement (preferred): {in: \"comp[.part]\", face: bottom|top|left|right|center, cover: 3, count: 2, side_cover: cover, axis: x|y, station: in}. bottom/top/left/right: bars at clear `cover` from that face, spread evenly between the zone's adjacent faces at `side_cover` (count 1 centers). center: bars centered in the zone on both axes (e.g. a single #4 in the middle of a stem wall); count > 1 spreads along axis x (default) or y at side_cover. station: ONE bar at that offset from the zone's left face (bottom face with axis y; for bottom/top/left/right faces it sets the along-face position)" },
            .{ .names = &.{"points"}, .def = "path: required", .desc = "polyline [x,y] or Refs; bends get radius bend_radius, drawn as fillets" },
            .{ .names = &.{"bend_radius"}, .def = "3*d_b", .desc = "inside bend radius for path bars" },
            .{ .names = &.{"spacing_note"}, .def = "null", .desc = "e.g. \"#4 @ 16\\\" O.C.\" for summaries and notes" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the bar (center = bar center for along_z)",
        .draws = "Default embedded:true. Cut dots draw solid; path bars draw as two parallel lines (bar outline) in pen rebar, filled solid when cut. 3D: swept circle. Natural z thickness = bar diameter. W_COVER is checked against the host concrete/cmu.",
        .example = "{\"id\":\"top_bar\",\"type\":\"rebar\",\"size\":\"#4\",\"at\":{\"anchor\":\"center\",\"to\":[6,-2.5]}}",
    },
    .{
        .type = .anchor_bolt,
        .summary = "Anchor bolt in the XY plane at a given z (shank, hook, nut and washer).",
        .params = &.{
            .{ .names = &.{"diameter"}, .def = "0.5", .desc = "0.5 or 0.625 typical" },
            .{ .names = &.{"embed"}, .def = "7 (4 for wedge/screw)", .desc = "length below the placement point (top of concrete); effective embedment for wedge/screw" },
            .{ .names = &.{"projection"}, .def = "2.5", .desc = "length above the placement point" },
            .{ .names = &.{"hook"}, .def = "J", .desc = "J: 180 degree bend toward +x, inside radius 1.5*d, returning up hook_len from the lowest point; L: 90 degree bend toward +x, horizontal leg ends hook_len from the shaft centerline; headed: square head 2*d wide, 0.5*d thick; none; wedge: post-installed expansion anchor (straight shaft, expansion clip 1.15*d wide x 0.6*embed long at the embedded end, nut+washer); screw: Titen HD style concrete screw (thread ticks along the embedment, hex washer head at the top, no nut)" },
            .{ .names = &.{"hook_len"}, .def = "J 2, L 3", .desc = "hook leg length in inches (see hook)" },
            .{ .names = &.{"nut_washer"}, .def = "true", .desc = "draw nut (1.5*d wide, 0.875*d tall, top at projection - 0.25*d) and washer (2.25*d wide, 0.125 thick) under it" },
        },
        .parts = "shank, nut, washer (wedge adds clip; screw has threads, washer, head instead of nut)",
        .anchors = "9 box anchors + top_of_concrete (where the bolt meets the host top surface, local (0,0))",
        .draws = "Embedded steel, natural z thickness = diameter. The placement anchor DEFAULTS to top_of_concrete, so `\"at\": {\"to\": \"cmu@top_center\"}` seats the bolt in the top surface, projection up (you do not set `anchor`). z defaults to the first section view's cut_z, so the bolt is cut, not hidden; set `z` only to move it. `array` {axis z, count, spacing} draws bolts @ spacing in iso/3D (one is cut in section).",
        .example = "{\"id\":\"anchor_bolt\",\"type\":\"anchor_bolt\",\"diameter\":0.5,\"embed\":7,\"projection\":2.75,\"hook\":\"J\",\"at\":{\"to\":[3,0]}}",
    },
    .{
        .type = .connector,
        .summary = "Schematic steel hardware: straps, ties, embedded anchors, drawn as a thickened polyline.",
        .params = &.{
            .{ .names = &.{"model"}, .def = "null", .desc = "e.g. MSTA36, H2.5A, HETA20, CS16, CS14: fills width/gauge from the hardware table" },
            .{ .names = &.{"points"}, .def = "required", .desc = "polyline [x,y] or Refs of the bearing face (lay edge) or centerline (lay face)" },
            .{ .names = &.{"lay"}, .def = "edge", .desc = "edge: seen edge-on, gauge in-plane growing to `side`, width along Z; face: seen face-on, `width` in-plane centered on the polyline, gauge along Z" },
            .{ .names = &.{"side"}, .def = "left", .desc = "lay edge: left of the polyline direction (left of a left-to-right line = up) or right" },
            .{ .names = &.{"gauge"}, .def = "18", .desc = "12 .1046, 14 .0747, 16 .0598, 18 .0478, 20 .0359" },
            .{ .names = &.{"width"}, .def = "1.25", .desc = "extent along Z (lay edge) or in-plane (lay face)" },
            .{ .names = &.{"fasteners"}, .def = "null", .desc = "text for notes, e.g. \"(10) 10d EA. END\"" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the resolved profile",
        .draws = "Pen steel; filled solid when cut, outline when beyond. Schematic: the note carries the model, the geometry shows location and path. `array` {axis z, count, spacing} draws the strap/tie @ spacing along the wall in iso/3D (section shows the cut one). Use named Refs (e.g. truss@heel_outer) in `points`, not literal offsets.",
        .example = "{\"id\":\"tie\",\"type\":\"connector\",\"model\":\"H2.5A\",\"lay\":\"face\",\"points\":[[0,0],[0,6]]}",
    },
    .{
        .type = .truss,
        .summary = "Prefab wood truss heel and tail in side view.",
        .params = &.{
            .{ .names = &.{"exterior"}, .def = "left", .desc = "side of the heel/overhang (right mirrors)" },
            .{ .names = &.{"pitch"}, .def = "4:12", .desc = "rise:run" },
            .{ .names = &.{"top_chord"}, .def = "2x4", .desc = "sawn nominal size, depth in-plane" },
            .{ .names = &.{"bottom_chord"}, .def = "2x4", .desc = "sawn nominal size, depth in-plane" },
            .{ .names = &.{"heel"}, .def = "standard", .desc = "standard | raised" },
            .{ .names = &.{"heel_height"}, .def = "null", .desc = "raised heel: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
            .{ .names = &.{"bearing_width"}, .def = "3.5", .desc = "width of the support under the heel" },
            .{ .names = &.{"overhang"}, .def = "12", .desc = "horizontal distance from outer face of bearing to the tail end" },
            .{ .names = &.{"tail"}, .def = "plumb", .desc = "plumb | square cut" },
            .{ .names = &.{"span_shown"}, .def = "48", .desc = "how far into the building to draw (crop/break at the end)" },
            .{ .names = &.{"plate"}, .def = "true", .desc = "draw the heel truss plate outline (dashed hidden pen)" },
        },
        .parts = "top_chord, bottom_chord, heel_web (raised only), plate, tail",
        .anchors = "9 box anchors + bearing_outer (local origin: outer edge of bearing at bottom of bottom chord), bearing_inner, tail_bottom, tail_top, top_chord_at_bearing, top_chord_bottom_at_bearing (lower edge of the top chord at the bearing plane), heel_outer (middle of the heel's outer vertical face, between the bottom chord's top and the top chord's top at the bearing: where ties and straps land), top_chord_end, bottom_chord_top_inner",
        .draws = "Standard heel: bottom chord from the bearing outer edge inward; top chord lower edge passes through (bearing_outer.x, top of bottom chord) at the pitch and extends to the plumb tail at x=-overhang. Members are in-plane, z thickness 1.5: set `z` and `array` for spacing.",
        .example = "{\"id\":\"truss\",\"type\":\"truss\",\"pitch\":\"4:12\",\"top_chord\":\"2x4\",\"bottom_chord\":\"2x4\",\"bearing_width\":7.25,\"overhang\":18,\"at\":{\"anchor\":\"bearing_outer\",\"to\":[0,0]}}",
    },
    .{
        .type = .membrane,
        .summary = "Thin layers: underlayment, vapor retarder, WRB, roofing, flashing.",
        .params = &.{
            .{ .names = &.{"material"}, .def = "membrane", .desc = "underlayment | vapor_retarder | wrb | shingles | flashing_membrane | membrane" },
            .{ .names = &.{"points"}, .def = "required", .desc = "polyline [x,y] or Refs" },
            .{ .names = &.{"thickness"}, .def = "per material", .desc = "draw thickness (vapor retarder 0.04, shingles 0.25 typical)" },
            .{ .names = &.{"side"}, .def = "left", .desc = "which side of the polyline direction the thickness grows: left of dx,dy is (-dy,dx)" },
            .{ .names = &.{"until"}, .def = "null", .desc = "a Ref (or {ref, offset}): the LAST segment grows or shrinks along its own direction until its end reaches the Ref's coordinate along that direction. With `slope`: \"@truss\" and points [[0,0],[12,0]] a roofing layer follows the roof and stops at e.g. \"truss@top_chord_end\"" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the resolved profile",
        .draws = "Drawn as a line per the material pen (vapor retarder: dashed heavy; shingles: heavy line with tick marks). Never occludes or hatches.",
        .example = "{\"id\":\"vapor\",\"type\":\"membrane\",\"material\":\"vapor_retarder\",\"points\":[[0,0],[48,0]]}",
    },
    .{
        .type = .fill,
        .summary = "Earth, gravel, sand, compacted fill as a hatched polygon.",
        .params = &.{
            .{ .names = &.{"material"}, .def = "earth", .desc = "earth | gravel | sand | compacted_fill" },
            .{ .names = &.{"points"}, .def = "required", .desc = "polygon [x,y] or Refs" },
            .{ .names = &.{"outline"}, .def = "top", .desc = "top (stroke only edges with outward normal up: the grade line) | full | none" },
            .{ .names = &.{"grade_label"}, .def = "null", .desc = "optional text for annotations" },
        },
        .parts = "none",
        .anchors = "9 box anchors of the polygon",
        .draws = "Hatched cut region; the hatch stops at the crop and fills never get break lines.",
        .example = "{\"id\":\"gravel\",\"type\":\"fill\",\"material\":\"gravel\",\"points\":[[0,0],[48,0],[48,-4],[0,-4]]}",
    },
    .{
        .type = .insulation,
        .summary = "Rigid (hatched) or batt (loop symbol) insulation.",
        .params = &.{
            .{ .names = &.{"form"}, .def = "rigid", .desc = "rigid | batt" },
            .{ .names = &.{ "width", "height" }, .def = "rect: required unless points", .desc = "box size" },
            .{ .names = &.{"points"}, .def = "null", .desc = "polygon alternative to width/height" },
        },
        .parts = "none",
        .anchors = "9 box anchors",
        .draws = "Rigid: hatched region. Batt: sinusoidal loop line fitted to the rectangle.",
        .example = "{\"id\":\"foam\",\"type\":\"insulation\",\"form\":\"rigid\",\"width\":2,\"height\":24}",
    },
    .{
        .type = .flashing,
        .summary = "Sheet-metal flashing in section: Z, L, drip edge, weep screed or free polyline.",
        .params = &.{
            .{ .names = &.{"profile"}, .def = "z", .desc = "z: back flange up the wall, horizontal leg out, drop at the nose; l: flange + horizontal leg; drip: flange on the deck, drop, outward kick; weep_screed: nailing flange up the wall, ledge, small drip drop; points: free centerline polyline" },
            .{ .names = &.{"flange"}, .def = "2 (weep_screed 3.5)", .desc = "vertical back/nailing flange length (drip: horizontal flange on the deck)" },
            .{ .names = &.{"leg"}, .def = "1 (l 2)", .desc = "horizontal leg length toward the exterior" },
            .{ .names = &.{"drop"}, .def = "2 (drip 1.5, weep_screed 0.5)", .desc = "downturned leg at the nose" },
            .{ .names = &.{"kick"}, .def = "0.5", .desc = "drip only: outward kick at the bottom of the drop" },
            .{ .names = &.{"gauge"}, .def = "26", .desc = "20 .0359, 22 .0299, 24 .0239, 26 .0179, 28 .0149" },
            .{ .names = &.{"exterior"}, .def = "left", .desc = "side the nose faces (right mirrors); presets only" },
            .{ .names = &.{"points"}, .def = "profile points: required", .desc = "centerline polyline [x,y] relative to the placement point, or Refs" },
        },
        .parts = "none",
        .anchors = "9 box anchors + corner (first bend, local (0,0) for presets), start, end",
        .draws = "Presets: corner at (0,0), wall surface x=0, exterior -x. Spans the document run along Z. Thin metal: solid fill + steel outline (SPEC 16). Embedded: drawn over hatch, exempt from W_OVERLAP. Place with at.anchor \"corner\".",
        .example = "{\"id\":\"pan\",\"type\":\"flashing\",\"profile\":\"z\",\"at\":{\"anchor\":\"corner\",\"to\":[0,0]}}",
    },
    .{
        .type = .joint,
        .summary = "Concrete joints: expansion filler strip, control (saw-cut) notch, tooled edge radius, sealant bead on backer rod.",
        .params = &.{
            .{ .names = &.{"kind"}, .def = "required", .desc = "expansion | control | tooled_edge | sealant" },
            .{ .names = &.{"width"}, .def = "0.5 (control 0.25)", .desc = "expansion: filler thickness; control: notch width at the top; sealant: joint gap width" },
            .{ .names = &.{"depth"}, .def = "expansion 4, control 1, sealant 0.25", .desc = "expansion: filler depth below the top (set to the slab thickness, or give `in`); control: notch depth (default 1/4 of the `in` zone height); sealant: bead depth" },
            .{ .names = &.{"in"}, .def = "null", .desc = "optional host zone \"comp[.part]\" whose height sets the default depth (expansion: full height; control: 1/4)" },
            .{ .names = &.{"cap"}, .def = "0", .desc = "expansion: depth of a sealant cap at the top of the filler (part `sealant`)" },
            .{ .names = &.{"radius"}, .def = "0.25", .desc = "tooled_edge: radius of the rounded corner" },
            .{ .names = &.{"corner"}, .def = "top_right", .desc = "tooled_edge: which corner of the concrete the point is: top_right (concrete lies left and below), top_left, bottom_right, bottom_left" },
            .{ .names = &.{"backer_rod"}, .def = "true", .desc = "sealant: draw the backer rod circle (diameter 1.25*width) below the bead" },
        },
        .parts = "expansion: filler (+ sealant with cap); control: notch; tooled_edge: radius; sealant: bead, rod",
        .anchors = "9 box anchors + joint_top (local (0,0): top surface at the joint centerline); tooled_edge adds corner",
        .draws = "Local origin: top surface, joint centerline (tooled_edge: the sharp corner). Expansion: filler strip width x depth with the joint_filler hatch (place it in the gap between two concrete pieces). Control and tooled_edge are void shapes (embedded: they cut a hole in the host hatch; not in 3D/iso). Sealant: bead (filled) over a backer rod circle. Spans the document run along Z.",
        .example = "{\"id\":\"ej\",\"type\":\"joint\",\"kind\":\"expansion\",\"width\":0.5,\"depth\":4,\"at\":{\"anchor\":\"joint_top\",\"to\":[0,0]}}",
    },
    .{
        .type = .solid,
        .summary = "Escape hatch: any extruded profile with an explicit material (flagged I_SOLID_USED).",
        .params = &.{
            .{ .names = &.{"profile"}, .def = "required", .desc = "{rect:[w,h]} | {circle:d} | {points:[...]}" },
            .{ .names = &.{"material"}, .def = "required", .desc = "any style material (aluminum, steel, ...)" },
        },
        .parts = "none",
        .anchors = "9 box anchors",
        .draws = "Use only when no typed component fits.",
        .example = "{\"id\":\"angle\",\"type\":\"solid\",\"material\":\"steel\",\"profile\":{\"rect\":[3,0.25]}}",
    },
};

comptime {
    const tags = std.meta.tags(Type);
    if (entries.len != tags.len) @compileError("catalog.entries must have exactly one entry per catalog.Type tag");
    for (entries, tags) |e, t| if (e.type != t) @compileError("catalog.entries must list the types in catalog.Type order; out of place: " ++ @tagName(e.type));
}

/// The entry of a type.
pub fn entry(t: Type) *const Entry {
    return &entries[@intFromEnum(t)];
}

pub fn find(name: []const u8) ?*const Entry {
    return entry(std.meta.stringToEnum(Type, name) orelse return null);
}

pub fn typeNames(a: Allocator) Allocator.Error![]const []const u8 {
    const out = try a.alloc([]const u8, entries.len);
    for (entries, 0..) |e, i| out[i] = e.name();
    return out;
}

/// Is `key` an accepted top-level key of a component of type `ty`?
pub fn allowedKey(ty: *const Entry, key: []const u8) bool {
    for (common) |c| for (c.names) |n| if (std.mem.eql(u8, n, key)) return true;
    for (ty.params) |p| for (p.names) |n| if (std.mem.eql(u8, n, key)) return true;
    return false;
}

/// Every accepted top-level key of a component of type `ty` (type params, then common fields).
pub fn allowedKeyNames(a: Allocator, ty: *const Entry) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    for (ty.params) |p| try out.appendSlice(a, p.names);
    for (common) |c| try out.appendSlice(a, c.names);
    return out.items;
}

pub fn allowedKeysText(a: Allocator, ty: *const Entry) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    var first = true;
    for (ty.params) |p| {
        for (p.names) |n| {
            if (!first) try out.appendSlice(a, ", ");
            first = false;
            try out.appendSlice(a, n);
        }
    }
    try out.appendSlice(a, ", + common: ");
    for (common, 0..) |c, i| {
        if (i > 0) try out.appendSlice(a, ", ");
        try out.appendSlice(a, c.names[0]);
    }
    return out.items;
}

// ---- rendering -----------------------------------------------------------------------------------------

fn paramJson(a: Allocator, p: Param) Allocator.Error!json.Value {
    return json.obj(a, &.{
        .{ .key = "name", .value = .{ .string = try p.nameText(a) } },
        .{ .key = "default", .value = .{ .string = p.def } },
        .{ .key = "desc", .value = .{ .string = p.desc } },
    });
}

pub fn entryJson(a: Allocator, e: *const Entry) Allocator.Error!json.Value {
    const ps = try a.alloc(json.Value, e.params.len);
    for (e.params, 0..) |p, i| ps[i] = try paramJson(a, p);
    if (e.type == .connector) {
        const ms = try a.alloc(json.Value, hardware.len);
        for (hardware, 0..) |h, i| ms[i] = .{ .string = try hardwareLine(a, h) };
        return json.obj(a, &.{
            .{ .key = "type", .value = .{ .string = e.name() } },
            .{ .key = "summary", .value = .{ .string = e.summary } },
            .{ .key = "params", .value = .{ .array = ps } },
            .{ .key = "models", .value = .{ .array = ms } },
            .{ .key = "parts", .value = .{ .string = e.parts } },
            .{ .key = "anchors", .value = .{ .string = e.anchors } },
            .{ .key = "draws", .value = .{ .string = e.draws } },
        });
    }
    return json.obj(a, &.{
        .{ .key = "type", .value = .{ .string = e.name() } },
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
    try out.print(a, "### `{s}`: {s}\n\n", .{ e.name(), e.summary });
    try out.appendSlice(a, "| param | default | notes |\n|---|---|---|\n");
    for (e.params) |p| try out.print(a, "| `{s}` | {s} | {s} |\n", .{ try p.nameText(a), p.def, p.desc });
    if (e.type == .connector) {
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
    for (common) |p| try out.print(a, "| `{s}` | {s} | {s} |\n", .{ try p.nameText(a), p.def, p.desc });
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
