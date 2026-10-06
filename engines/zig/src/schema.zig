//! Field schemas of the non-component objects (SPEC 19): the single source of truth for
//!   - `kerf schema <topic>` and the schema section of `kerf guide`,
//!   - `W_UNKNOWN_KEY` (lint.zig) and the canonical key order (canon.zig),
//!   - the minimal example document (`kerf new --template section`, the guide).
//! Component types come from catalog.zig (the same table validates their params); `kerf schema <type>` renders that.

const std = @import("std");
const json = @import("json.zig");
const catalog = @import("catalog.zig");
const canon = @import("canon.zig");
const fields = @import("schema_fields.zig");
pub const Field = fields.Field;
pub const Object = fields.Object;
pub const objects = fields.objects;
const Allocator = std.mem.Allocator;

// ---- the minimal example document ---------------------------------------------------------------------------

/// Two components, a section view with crop/scale/notes_side, two notes (one citation), one dim, one label.
/// `kerf new --template section` writes this (renamed); the guide embeds it. It checks with 0 errors and 0 warnings.
pub const example_doc =
    \\{"kerf":"0.1","id":"wall-at-sill","title":"WALL AT PT SILL","meta":{"jurisdiction":{"code":"IRC","edition":2021}},"run":[-24,24],
    \\"components":[
    \\{"id":"sill","type":"lumber","size":"2x6","orient":"flat","treated":true,"at":{"anchor":"bottom_left","to":[0,0]}},
    \\{"id":"stud","type":"lumber","size":"2x6","run":"y","length":24,"at":{"anchor":"bottom_left","to":"sill@top_left"}}],
    \\"views":[{"id":"A","kind":"section","number":"1","title":"WALL AT PT SILL","scale":"1\"=1'-0\"","cut_z":0,"crop":{"x":[-12,18],"y":[-8,30]},"notes_side":"both",
    \\"annotations":[
    \\{"id":"n_sill","type":"note","text":"2X6 PT SILL PLATE","target":"sill","cite":[{"code":"IRC","edition":2021,"section":"R317.1","title":"Location required","status":"suggested"}]},
    \\{"id":"n_stud","type":"note","text":"2X6 STUDS @ 16\" O.C.","target":"stud"},
    \\{"id":"d_sill","type":"dim","from":"sill@bottom_left","to":"sill@bottom_right","dir":"h","offset":-3},
    \\{"id":"l_ext","type":"label","text":"EXTERIOR","at":"sill@top_left","offset":[-6,6]}]}]}
;

/// Canonical text of the example document with a new id and title (`kerf new --template section`).
pub fn templateText(a: Allocator, id: []const u8, title: []const u8) ![]u8 {
    var perr: json.ParseError = undefined;
    var v = (try json.parse(a, example_doc, &perr)) orelse return error.BadTemplate;
    var ms: std.ArrayList(json.Member) = .empty;
    for (v.object) |m| {
        if (std.mem.eql(u8, m.key, "id")) {
            try ms.append(a, .{ .key = "id", .value = .{ .string = id } });
        } else if (std.mem.eql(u8, m.key, "title") and title.len > 0) {
            try ms.append(a, .{ .key = "title", .value = .{ .string = title } });
        } else try ms.append(a, m);
    }
    v = .{ .object = ms.items };
    return canon.write(a, v);
}

// ---- rendering ----------------------------------------------------------------------------------------------

fn fieldLine(out: *std.ArrayList(u8), a: Allocator, f: Field) Allocator.Error!void {
    try out.print(a, "- {s}: {s}, ", .{ f.name, f.ty });
    if (f.required) {
        try out.appendSlice(a, "required");
    } else if (f.def.len > 0) {
        try out.print(a, "default {s}", .{f.def});
    } else try out.appendSlice(a, "optional");
    try out.print(a, ". {s}\n", .{f.desc});
}

fn appendExample(out: *std.ArrayList(u8), a: Allocator, text: []const u8) Allocator.Error!void {
    if (text.len == 0) return;
    try out.print(a, "\nExample:\n{s}\n", .{text});
}

fn renderObject(a: Allocator, o: *const Object) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.print(a, "{s}: {s}\n\n", .{ o.name, o.summary });
    for (o.fields) |f| try fieldLine(&out, a, f);
    if (o.notes.len > 0) try out.print(a, "\n{s}\n", .{o.notes});
    if (std.mem.eql(u8, o.name, "doc")) {
        try out.appendSlice(a, "\nMinimal complete document (`kerf new x.kerf.json --template section` writes it):\n");
        try out.appendSlice(a, try exampleDocText(a));
    } else try appendExample(&out, a, o.example);
    return out.items;
}

/// The example document, canonical (pretty) text.
pub fn exampleDocText(a: Allocator) Allocator.Error![]const u8 {
    var perr: json.ParseError = undefined;
    const v = (try json.parse(a, example_doc, &perr)) orelse return example_doc;
    return canon.write(a, v);
}

fn renderCommon(a: Allocator) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "common: fields every component accepts (plus the type's own params: `kerf schema <type>`).\n\n");
    for (catalog.common) |p| {
        try out.print(a, "- {s}: ", .{try p.nameText(a)});
        if (std.mem.eql(u8, p.def, "required")) try out.appendSlice(a, "required") else try out.print(a, "default {s}", .{p.def});
        try out.print(a, ". {s}\n", .{p.desc});
    }
    try out.appendSlice(a, "\nTypes: ");
    for (catalog.entries, 0..) |e, i| {
        if (i > 0) try out.appendSlice(a, ", ");
        try out.appendSlice(a, e.name());
    }
    try out.appendSlice(a, "\n");
    return out.items;
}

fn renderComponent(a: Allocator, e: *const catalog.Entry) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.print(a, "{s}: {s}\n\n", .{ e.name(), e.summary });
    for (e.params) |p| {
        const req = std.mem.startsWith(u8, p.def, "required") or std.mem.endsWith(u8, p.def, ": required");
        try out.print(a, "- {s}: ", .{try p.nameText(a)});
        if (req) try out.print(a, "{s}", .{p.def}) else try out.print(a, "default {s}", .{p.def});
        try out.print(a, ". {s}\n", .{p.desc});
    }
    try out.print(a, "\nCommon fields (every type): ", .{});
    for (catalog.common, 0..) |p, i| {
        if (i > 0) try out.appendSlice(a, ", ");
        try out.appendSlice(a, p.names[0]);
    }
    try out.appendSlice(a, " (`kerf schema common`).\n");
    try out.print(a, "Parts: {s}\nAnchors: {s}\nDraws: {s}\n", .{ e.parts, e.anchors, e.draws });
    if (e.traits.hardware) {
        try out.appendSlice(a, "Hardware models: ");
        for (catalog.hardware, 0..) |h, i| {
            if (i > 0) try out.appendSlice(a, ", ");
            try out.appendSlice(a, h.model);
        }
        try out.appendSlice(a, " (`kerf catalog` lists widths and gauges)\n");
    }
    try appendExample(&out, a, e.example);
    return out.items;
}

const refs_text =
    \\refs: how components name points (anywhere a point is accepted: `at.to`, `until`, note `at`, dim `from`/`to`, label `at`, point lists).
    \\
    \\- "comp@anchor" e.g. "sill@top_left"; "comp.part@anchor" e.g. "truss.top_chord@top_right"; "@origin" is (0, 0).
    \\- "comp#k@anchor" addresses instance k of an array (`kerf schema array`).
    \\- {"ref": "footing@bottom_left", "offset": [3, 3]} adds an offset; a literal point is [x, y] in inches (lengths also accept "3'-4 1/2\"").
    \\- Anchors: the 9 box anchors top_left top_center top_right middle_left center middle_right bottom_left bottom_center bottom_right,
    \\  plus named anchors per type (`kerf schema <type>`; `kerf call inspect` with {"q":"anchors","id":"<comp>"} lists them with coordinates).
    \\- Coordinates: inches, X right, Y up, Z toward the viewer; sections look along -Z.
    \\
;

pub const topics_hint = "doc view note dim label cite ops at array acknowledge common refs <component type>";

/// Text for `kerf schema <topic>`; null when the topic is unknown.
pub fn render(a: Allocator, topic: []const u8) Allocator.Error!?[]const u8 {
    if (fields.findObject(topic)) |o| return try renderObject(a, o);
    if (catalog.find(topic)) |e| return try renderComponent(a, e);
    const eq = std.mem.eql;
    if (eq(u8, topic, "common") or eq(u8, topic, "component") or eq(u8, topic, "components")) return try renderCommon(a);
    if (eq(u8, topic, "refs") or eq(u8, topic, "ref") or eq(u8, topic, "anchors") or eq(u8, topic, "anchor")) return refs_text;
    if (eq(u8, topic, "annotation") or eq(u8, topic, "annotations")) {
        var out: std.ArrayList(u8) = .empty;
        for ([_]*const Object{ &fields.note, &fields.dim, &fields.label }) |o| {
            try out.appendSlice(a, try renderObject(a, o));
            try out.append(a, '\n');
        }
        return out.items;
    }
    return null;
}

/// Topic list (`kerf schema` with no topic).
pub fn index(a: Allocator) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "kerf schema <topic>: field reference (required/optional, types, defaults, one example). Topics:\n\n");
    for (objects) |o| try out.print(a, "  {s: <12} {s}\n", .{ o.name, o.brief });
    try out.print(a, "  {s: <12} {s}\n", .{ "common", "fields every component accepts" });
    try out.print(a, "  {s: <12} {s}\n", .{ "refs", "how to name points and anchors" });
    for (catalog.entries) |e| try out.print(a, "  {s: <12} {s}\n", .{ e.name(), firstSentence(e.summary) });
    return out.items;
}

fn firstSentence(s: []const u8) []const u8 {
    if (std.mem.indexOf(u8, s, ". ")) |i| return s[0 .. i + 1];
    return s;
}

/// The schema section embedded in `kerf guide`: view, note, dim, label, cite, ops, then the example document.
pub fn guideSection(a: Allocator) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "# Document schema (generated: `kerf schema <topic>`)\n\n");
    try out.appendSlice(a, "Topics: ");
    try out.appendSlice(a, topics_hint);
    try out.appendSlice(a, ". Unknown keys are kept but warn (W_UNKNOWN_KEY, with a suggestion): the field names below are the only ones.\n\n");
    for ([_]*const Object{ &fields.view, &fields.note, &fields.dim, &fields.label, &fields.cite, &fields.op }) |o| {
        try out.appendSlice(a, "```text\n");
        try out.appendSlice(a, try renderObject(a, o));
        try out.appendSlice(a, "```\n\n");
    }
    try out.appendSlice(a, "Complete minimal document (two components, a section view, two notes with one citation, a dimension, a label). `kerf new x.kerf.json --template section` writes it:\n\n```json\n");
    try out.appendSlice(a, try exampleDocText(a));
    try out.appendSlice(a, "```\n\n");
    return out.items;
}

fn compactLine(out: *std.ArrayList(u8), a: Allocator, o: *const Object) Allocator.Error!void {
    try out.print(a, "- {s}:", .{o.name});
    for (o.fields) |f| {
        try out.print(a, " {s}", .{f.name});
        if (f.required) try out.append(a, '*');
        if (f.ty.len <= 24 and !std.mem.eql(u8, f.ty, "string")) try out.print(a, ":{s}", .{f.ty});
        if (!f.required and f.def.len > 0 and f.def.len <= 12) try out.print(a, "={s}", .{f.def});
        try out.append(a, ',');
    }
    out.items.len -= 1; // trailing comma
    try out.append(a, '\n');
}

/// The schema section of the short `kerf guide`: one line per object (field names, `*` = required, `:type`, `=default`),
/// then the complete minimal example document and the topic list. `kerf schema <topic>` has the descriptions.
pub fn compactSection(a: Allocator) Allocator.Error![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "## Document schema (field names are exact; `*` required, `:type`, `=default`; descriptions: `kerf schema <topic>`)\n");
    for ([_]*const Object{ &fields.doc, &fields.view, &fields.note, &fields.dim, &fields.label, &fields.cite }) |o| try compactLine(&out, a, o);
    try out.appendSlice(a, "- annotations go in a view's `annotations`; note `target` is a component id (or `comp.part`), `at`/dim `from`/`to` are Refs or [x,y]\n\n");
    try out.appendSlice(a, "## Complete example (two components, a section view, two notes with one citation, a dim, a label; `kerf new x.kerf.json --template section` writes it)\n```json\n");
    try out.appendSlice(a, example_doc);
    try out.appendSlice(a, "\n```\n\n## Topics: `kerf schema <topic> [<topic> ...]`\n");
    try out.appendSlice(a, topics_hint);
    try out.appendSlice(a, "\nComponent types (`kerf schema <type>` = params, anchors, example):\n");
    for (catalog.entries) |e| try out.print(a, "- {s}: {s}\n", .{ e.name(), firstSentence(e.summary) });
    return out.items;
}

test "schema topics render and the example document is valid" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    for (objects) |o| {
        const t = (try render(a, o.name)).?;
        try std.testing.expect(std.mem.indexOf(u8, t, o.fields[0].name) != null);
        for (t) |c| try std.testing.expect(c < 0x80);
    }
    for (catalog.entries) |*e| {
        const t = (try render(a, e.name())).?;
        try std.testing.expect(std.mem.indexOf(u8, t, "Example:") != null);
    }
    try std.testing.expect((try render(a, "nope")) == null);
    const g = try guideSection(a);
    for (g) |c| try std.testing.expect(c < 0x80);
    try std.testing.expect(std.mem.indexOf(u8, g, "notes_side") != null);
}
