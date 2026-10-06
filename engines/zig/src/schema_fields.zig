//! Field tables of the non-component objects (SPEC 19): doc, view, note, dim, label, cite, ops, at, array, acknowledge.
//! Pure data plus lookups; `schema.zig` renders them (`kerf schema`, the guide), `lint.zig` checks documents against them
//! (W_UNKNOWN_KEY) and `canon.zig` takes the canonical key order from them. It imports nothing from the engine, so the
//! renderer (which needs `canon.write`) and the canonicalizer (which needs the key order) no longer import each other.

const std = @import("std");
const Allocator = std.mem.Allocator;

pub const Field = struct {
    name: []const u8,
    /// Short type text, e.g. `string`, `[x, y]`, `right|left|both`.
    ty: []const u8,
    /// Empty = optional with no stated default.
    def: []const u8 = "",
    required: bool = false,
    desc: []const u8,
};

pub const Object = struct {
    name: []const u8,
    /// Few words for the topic list.
    brief: []const u8,
    summary: []const u8,
    fields: []const Field,
    /// Extra text printed after the fields (path tables, rules).
    notes: []const u8 = "",
    /// One compact JSON example ("" = none; the doc example is rendered from `example_doc`).
    example: []const u8 = "",
};

// ---- the objects ------------------------------------------------------------------------------------------

pub const doc: Object = .{
    .name = "doc",
    .brief = "the whole document",
    .summary = "The whole document (one `<id>.kerf.json` file).",
    .fields = &.{
        .{ .name = "kerf", .ty = "string", .required = true, .desc = "schema version, always \"0.1\"" },
        .{ .name = "id", .ty = "string", .required = true, .desc = "slug of the detail, e.g. \"truss-bearing-cmu\"" },
        .{ .name = "title", .ty = "string", .def = "\"\"", .desc = "sheet title in UPPERCASE" },
        .{ .name = "meta", .ty = "object", .desc = "free-form, known keys: requested[] (the designer's asks; see below), author, discipline, classification {uniformat, masterformat[]}, jurisdiction {code, edition} (default citation basis, e.g. {\"code\":\"IRC\",\"edition\":2021}), tags[], sheet, date, forked_from" },
        .{ .name = "run", .ty = "[z0, z1]", .def = "[-24, 24]", .desc = "default z extent (inches) of members that span the depth; members with a natural z thickness are centered on the section cut (or the middle of run)" },
        .{ .name = "components", .ty = "[component]", .def = "[]", .desc = "the construction; order = draw order (see `kerf schema <type>` and `kerf catalog`)" },
        .{ .name = "views", .ty = "[view]", .def = "[]", .desc = "drawings of the components (see `kerf schema view`)" },
    },
    .notes = "First build: one op {\"op\":\"set\",\"path\":\"doc\",\"value\":<whole document>}. Then small add/update/remove ops (`kerf schema ops`).\n" ++
        "meta.requested: [\"cmu wall\", \"bond beam\", \"H2.5A ties\"]: the designer's asks, one short phrase each, written on the first build. `kerf check` and `kerf apply` print a COVERAGE block " ++
        "(`ok <component ids>` or `MISSING` per item) and W_REQUESTED_MISSING for each missing item. An item is covered when all its words (case-insensitive, plural s ignored) appear in one component's " ++
        "id/type/label/model/size or in one note/label text, so name components and write notes with the designer's words. Finish only at full coverage; remove an item only if the designer dropped it.",
};

pub const view: Object = .{
    .name = "view",
    .brief = "a drawing view (crop, scale, cut_z, notes_side, annotations)",
    .summary = "One drawing view (goes in `views[]`; export with `--view <id>`).",
    .fields = &.{
        .{ .name = "id", .ty = "string", .required = true, .desc = "view id, e.g. \"A\"; used by `kerf export --view A` and op paths `views/A/...`" },
        .{ .name = "kind", .ty = "section|iso", .def = "section", .desc = "section = 2D cut looking toward -Z; iso = hidden-line 3D" },
        .{ .name = "number", .ty = "string", .def = "order in views: \"1\", \"2\"", .desc = "detail number in the title bubble" },
        .{ .name = "title", .ty = "string", .def = "\"\"", .desc = "title under the view, UPPERCASE, e.g. \"TRUSS BEARING AT CMU WALL\"" },
        .{ .name = "scale", .ty = "string", .desc = "section: 3\"=1'-0\" | 1-1/2\"=1'-0\" | 1\"=1'-0\" | 3/4\"=1'-0\" | 1/2\"=1'-0\" | 3/8\"=1'-0\" | 1/4\"=1'-0\" | \"1:N\"; iso: \"NTS\". When omitted the engine picks the largest standard scale at which the view + notes + title fit the frame" },
        .{ .name = "cut_z", .ty = "length", .def = "0", .desc = "section: z of the cut plane; members whose z range contains it are cut (hatched/marked), others are beyond or hidden. Also the default z of in-plane members and anchor bolts" },
        .{ .name = "crop", .ty = "{\"x\":[x0,x1],\"y\":[y0,y1]}", .desc = "model-inch window (x right, y up). When omitted it auto-fits all non-fill components plus 6\" (fills are clipped to that box). Solids cut by the window get break lines. After an edit that leaves a member mostly outside an explicit crop, W_CROP_STALE says so (remove crop and scale to re-fit)" },
        .{ .name = "from", .ty = "front_right|front_left|back_right|back_left", .def = "front_right", .desc = "iso only: viewing corner" },
        .{ .name = "cutaway", .ty = "boolean", .def = "false", .desc = "iso only: clip solids to z <= cut_z and hatch the cut face" },
        .{ .name = "notes_side", .ty = "right|left|both", .def = "both", .desc = "which side the note column goes; both = each note goes to the side nearer its landing point" },
        .{ .name = "omit", .ty = "[component id]", .def = "[]", .desc = "components hidden in this view only" },
        .{ .name = "annotations", .ty = "[note|dim|label]", .def = "[]", .desc = "notes, dimensions, labels in drawing order (`kerf schema note|dim|label`)" },
    },
    .example = "{\"id\":\"A\",\"kind\":\"section\",\"number\":\"1\",\"title\":\"WALL AT SILL\",\"scale\":\"1\\\"=1'-0\\\"\",\"cut_z\":0,\"crop\":{\"x\":[-12,18],\"y\":[-8,30]},\"notes_side\":\"both\",\"annotations\":[]}",
};

pub const note: Object = .{
    .name = "note",
    .brief = "leader note with optional citations",
    .summary = "Leader note (annotation of type note): UPPERCASE text, an arrow landing inside the target, optional code citations.",
    .fields = &.{
        .{ .name = "id", .ty = "string", .required = true, .desc = "unique within the view, [a-z][a-z0-9_]*" },
        .{ .name = "type", .ty = "\"note\"", .required = true, .desc = "annotation type" },
        .{ .name = "text", .ty = "string", .required = true, .desc = "house format: UPPERCASE, no trailing period, fractions `1 1/2\"`, ` X ` between sizes, abbreviations W/ O.C. EA. CONT. TYP. MIN. DIA. BOTT. CONC. GYP. BD. REINF. PT (W_NOTE_STYLE lints this, and also commentary such as `?`, `NOTE:`, `WE`, `SHOULD BE`, a text that ends on `W/` or `AND`, over 130 characters, a repeat of another note)" },
        .{ .name = "target", .ty = "string", .desc = "component id or `comp.part` (e.g. `slab.footing`); the arrow lands inside its visible region. Required unless `at` is given" },
        .{ .name = "at", .ty = "point", .desc = "exact arrow landing: \"comp@anchor\", {\"ref\":\"comp@anchor\",\"offset\":[dx,dy]} or [x, y]. Use it when the target is hidden or the label point is bad" },
        .{ .name = "place", .ty = "[x, y]", .desc = "model-inch position of the text (designer override; the note is then fixed). Not `side`/`pos`: the column side is the VIEW's `notes_side`" },
        .{ .name = "column", .ty = "left|right", .desc = "which notes column this note's text goes in (per-note version of the view's `notes_side`; the layout never moves it to the other column). A note with `place` right-aligns its text when it sits left of its arrow" },
        .{ .name = "cite", .ty = "[cite]", .desc = "code citations printed after the text, e.g. (IRC R403.1.6)* (`kerf schema cite`). The key is `cite`, not citations" },
    },
    .example = "{\"id\":\"n_sill\",\"type\":\"note\",\"text\":\"2X6 PT SILL PLATE\",\"target\":\"sill\",\"cite\":[{\"code\":\"IRC\",\"edition\":2021,\"section\":\"R317.1\",\"title\":\"Location required\",\"status\":\"suggested\"}]}",
};

pub const dim: Object = .{
    .name = "dim",
    .brief = "linear dimension",
    .summary = "Linear dimension between two points (annotation of type dim). Section views only.",
    .fields = &.{
        .{ .name = "id", .ty = "string", .required = true, .desc = "unique within the view" },
        .{ .name = "type", .ty = "\"dim\"", .required = true, .desc = "annotation type" },
        .{ .name = "from", .ty = "point", .required = true, .desc = "\"comp@anchor\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y]" },
        .{ .name = "to", .ty = "point", .required = true, .desc = "same forms as `from`" },
        .{ .name = "dir", .ty = "h|v|aligned", .def = "dominant axis", .desc = "h measures |dx|, v measures |dy|, aligned the true distance. When omitted: h if |dx| >= |dy|, else v. W_DIM_ZERO fires when the dimension measures under 1/16\"" },
        .{ .name = "offset", .ty = "length", .def = "0", .desc = "distance of the dimension line from the points, model inches. h: > 0 puts it above the higher point, < 0 below the lower; v: > 0 right of the rightmost point, < 0 left of the leftmost; aligned: perpendicular, sign = side" },
        .{ .name = "text", .ty = "string", .desc = "replaces the measured text (e.g. \"VERIFY\")" },
    },
    .example = "{\"id\":\"d_sill\",\"type\":\"dim\",\"from\":\"sill@bottom_left\",\"to\":\"sill@bottom_right\",\"dir\":\"h\",\"offset\":-3}",
};

pub const label: Object = .{
    .name = "label",
    .brief = "free text without a leader",
    .summary = "Free text without a leader (annotation of type label), such as EXTERIOR, INTERIOR or GRADE.",
    .fields = &.{
        .{ .name = "id", .ty = "string", .required = true, .desc = "unique within the view" },
        .{ .name = "type", .ty = "\"label\"", .required = true, .desc = "annotation type" },
        .{ .name = "text", .ty = "string", .required = true, .desc = "UPPERCASE text" },
        .{ .name = "at", .ty = "point", .required = true, .desc = "\"comp@anchor\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y]" },
        .{ .name = "offset", .ty = "[dx, dy]", .def = "[0, 0]", .desc = "model inches added to `at`" },
    },
    .example = "{\"id\":\"l_ext\",\"type\":\"label\",\"text\":\"EXTERIOR\",\"at\":\"sill@top_left\",\"offset\":[-6,6]}",
};

pub const cite: Object = .{
    .name = "cite",
    .brief = "one code citation",
    .summary = "One code citation; goes in a note's `cite` array.",
    .fields = &.{
        .{ .name = "code", .ty = "string", .required = true, .desc = "IRC, IBC, ACI 318, TMS 402, ..." },
        .{ .name = "edition", .ty = "number", .def = "meta.jurisdiction.edition", .desc = "e.g. 2021" },
        .{ .name = "section", .ty = "string", .required = true, .desc = "e.g. \"R403.1.6\". Only cite sections you are sure exist" },
        .{ .name = "title", .ty = "string", .desc = "short section title" },
        .{ .name = "status", .ty = "suggested|verified", .def = "suggested", .desc = "agents may only write suggested; only the designer verifies (an agent's verified is reset). Unverified citations print with a trailing *" },
    },
    .example = "{\"code\":\"IRC\",\"edition\":2021,\"section\":\"R403.1.6\",\"title\":\"Foundation anchorage\",\"status\":\"suggested\"}",
};

pub const op: Object = .{
    .name = "ops",
    .brief = "the edit ops `kerf apply` takes",
    .summary = "The edit list `kerf apply` takes: a JSON array of op objects, applied in order and atomically (any error: nothing is written).",
    .fields = &.{
        .{ .name = "op", .ty = "add|update|remove|set", .required = true, .desc = "what to do" },
        .{ .name = "path", .ty = "string", .required = true, .desc = "where (table below)" },
        .{ .name = "value", .ty = "json", .desc = "the object to add / the merge patch / the replacement; required except for remove" },
        .{ .name = "before", .ty = "component id", .desc = "add to `components` only: insert before this component instead of appending" },
    },
    .notes =
    \\Paths:
    \\  add    components                       value = a component object (unique id)
    \\  add    views                            value = a view object
    \\  add    views/<view>/annotations         value = a note | dim | label
    \\  update components/<id>                  value = JSON merge patch: given keys replace (arrays and objects whole), null deletes a key; `id` cannot change
    \\  update views/<view>                     merge patch of view fields (not `annotations`: use the annotation paths)
    \\  update views/<view>/annotations/<id>    merge patch (changing `text` or `cite` resets citations to suggested)
    \\  update meta                             merge patch
    \\  remove components/<id> | views/<view> | views/<view>/annotations/<id>
    \\          (a component still referenced by an `at`, `until`, `slope`, note target or dim is refused: the error names the referrers)
    \\  set    doc                              value = the whole document (first build)
    \\  set    components/<id>                  replace the whole component
    \\Input forms: `kerf apply <doc> ops.json`, `... -` (stdin), or `--ops '<json>'`; the ops may be an array, a single op object, or {"ops":[...],"why":"..."} (the why is used when --why is absent). Add `-w --why "..."` to write and log.
    ,
    .example = "[{\"op\":\"add\",\"path\":\"components\",\"value\":{\"id\":\"sill\",\"type\":\"lumber\",\"size\":\"2x6\",\"orient\":\"flat\",\"treated\":true}},{\"op\":\"update\",\"path\":\"views/A/annotations/n_sill\",\"value\":{\"text\":\"2X6 PT SILL PLATE W/ SEALER\"}},{\"op\":\"remove\",\"path\":\"components/old\"}]",
};

pub const at: Object = .{
    .name = "at",
    .brief = "component placement",
    .summary = "Component placement (`at`): translate the member so its own anchor lands on a point.",
    .fields = &.{
        .{ .name = "anchor", .ty = "anchor name", .def = "bottom_left (anchor_bolt: top_of_concrete)", .desc = "which anchor of THIS component lands on `to`: the 9 box anchors or a builder's named anchor" },
        .{ .name = "to", .ty = "point", .def = "[0, 0]", .desc = "\"comp@anchor\", \"comp.part@anchor\", \"@origin\", {\"ref\":...,\"offset\":[dx,dy]} or [x, y]" },
        .{ .name = "offset", .ty = "[dx, dy]", .def = "[0, 0]", .desc = "model inches added to `to`" },
    },
    .example = "{\"anchor\":\"bottom_left\",\"to\":\"sill@top_left\",\"offset\":[0,0]}",
};

pub const array: Object = .{
    .name = "array",
    .brief = "repeat a component",
    .summary = "Repeat a component (`array`): instance k is translated by k*spacing. Instances are id#0..id#n-1; a ref to `id` means instance 0, `id#k@anchor` addresses instance k.",
    .fields = &.{
        .{ .name = "axis", .ty = "x|y|z", .def = "x", .desc = "x/y translate in the section plane; z steps in depth (trusses, anchor bolts, straps along the wall: drawn in iso/3D, one cut instance in section)" },
        .{ .name = "count", .ty = "integer 1..500", .def = "1", .desc = "number of instances" },
        .{ .name = "spacing", .ty = "length", .required = true, .desc = "inches between instances (may be negative)" },
    },
    .example = "{\"axis\":\"z\",\"count\":3,\"spacing\":32}",
};

pub const ack: Object = .{
    .name = "acknowledge",
    .brief = "suppress a warning with a logged reason",
    .summary = "`acknowledge` on a component: suppress a warning for that component with a reason that is logged.",
    .fields = &.{
        .{ .name = "code", .ty = "string", .required = true, .desc = "the warning code, e.g. W_UNTREATED_CONTACT, W_OVERLAP, W_NEAR_MISS, W_SHORT_SLOPE, W_COVER" },
        .{ .name = "reason", .ty = "string", .required = true, .desc = "why it is fine; prints in the summary as an I_ACK line and is logged by `kerf apply -w`" },
    },
    .notes = "Usage: \"acknowledge\": [{\"code\": \"W_UNTREATED_CONTACT\", \"reason\": \"truss seat moisture barrier by mfr.\"}] on the component the warning names. Errors (E_*) cannot be acknowledged; an I_* info code is accepted and ignored (no error, nothing to suppress). Prefer fixing the cause; for wood on masonry use `\"barrier\": \"sill_seal\"` (lumber) instead.",
    .example = "[{\"code\":\"W_UNTREATED_CONTACT\",\"reason\":\"truss seat moisture barrier by mfr.\"}]",
};

/// Every non-component topic, in listing order.
pub const objects = [_]*const Object{ &doc, &view, &note, &dim, &label, &cite, &op, &at, &array, &ack };

pub fn findObject(name: []const u8) ?*const Object {
    for (objects) |o| if (std.mem.eql(u8, o.name, name)) return o;
    if (std.mem.eql(u8, name, "op")) return &op;
    if (std.mem.eql(u8, name, "ack")) return &ack;
    return null;
}

pub fn hasField(o: *const Object, key: []const u8) bool {
    for (o.fields) |f| if (std.mem.eql(u8, f.name, key)) return true;
    return false;
}

pub fn fieldNames(a: Allocator, o: *const Object) Allocator.Error![]const []const u8 {
    const out = try a.alloc([]const u8, o.fields.len);
    for (o.fields, 0..) |f, i| out[i] = f.name;
    return out;
}

/// Comptime key list of an object (canonical key order for canon.zig).
pub fn keys(comptime o: *const Object) [o.fields.len][]const u8 {
    var out: [o.fields.len][]const u8 = undefined;
    for (o.fields, 0..) |f, i| out[i] = f.name;
    return out;
}
