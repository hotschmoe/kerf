//! Document lints (SPEC 19): W_UNKNOWN_KEY, W_NOTE_STYLE, W_DIM_ZERO, `acknowledge` handling (I_ACK),
//! and the dim `dir` default. They look at the document and the compiled scene, not at geometry output;
//! field names come from schema.zig / catalog.zig (the same tables the parsers and `kerf schema` use).

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const schema = @import("schema.zig");
const scene_mod = @import("scene.zig");
const units = @import("units.zig");
const coverage = @import("coverage.zig");
const Allocator = std.mem.Allocator;
const Scene = scene_mod.Scene;

pub fn run(a: Allocator, scene: *Scene, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    if (doc != .object) return;
    try unknownKeys(a, doc, diags);
    try noteStyle(a, doc, diags);
    try dimZero(a, scene, doc, diags);
    try ackShape(a, scene, doc, diags);
    try requestedMissing(a, doc, diags);
}

// ---- W_UNKNOWN_KEY --------------------------------------------------------------------------------------------------

const Syn = struct {
    key: []const u8,
    to: []const u8,
    /// Object names the row applies to (empty = any).
    objs: []const []const u8 = &.{},
    /// Full replacement for the suggestion text.
    hint: []const u8 = "",
};

const annotation_objs = [_][]const u8{ "note", "dim", "label" };

const synonyms = [_]Syn{
    .{ .key = "citations", .to = "cite" },
    .{ .key = "citation", .to = "cite" },
    .{ .key = "cites", .to = "cite" },
    .{ .key = "side", .to = "notes_side", .objs = &.{"view"} },
    .{ .key = "side", .to = "notes_side", .objs = &annotation_objs, .hint = "the note column side is the VIEW's \"notes_side\" (right|left|both, at views/<view>/notes_side); to put one note's text somewhere use \"place\": [x, y], to move its arrow use \"at\"" },
    .{ .key = "point", .to = "at" },
    .{ .key = "target_point", .to = "at" },
    .{ .key = "arrow", .to = "at" },
    .{ .key = "kind", .to = "type", .objs = &annotation_objs },
    .{ .key = "pos", .to = "place" },
    .{ .key = "position", .to = "place" },
    .{ .key = "ref", .to = "to", .objs = &.{"at"} },
};

fn suggestKey(a: Allocator, o: *const schema.Object, key: []const u8) ?Syn {
    for (synonyms) |s| {
        if (!std.mem.eql(u8, s.key, key)) continue;
        if (s.objs.len > 0) {
            var hit = false;
            for (s.objs) |n| if (std.mem.eql(u8, n, o.name)) {
                hit = true;
            };
            if (!hit) continue;
        }
        return s;
    }
    // nearest valid key by edit distance (typos: "ofset", "annotation")
    var best: ?[]const u8 = null;
    var bd: usize = std.math.maxInt(usize);
    for (o.fields) |f| {
        const d = model.editDistance(a, key, f.name);
        if (d < bd) {
            bd = d;
            best = f.name;
        }
    }
    const limit: usize = if (key.len <= 4) 1 else 2;
    if (best != null and bd <= limit) return .{ .key = key, .to = best.? };
    return null;
}

fn keyList(a: Allocator, o: *const schema.Object) []const u8 {
    var out: std.ArrayList(u8) = .empty;
    for (o.fields, 0..) |f, i| {
        if (i > 0) out.appendSlice(a, ", ") catch {};
        out.appendSlice(a, f.name) catch {};
    }
    return out.items;
}

fn lintObject(a: Allocator, diags: *model.Diags, v: json.Value, o: *const schema.Object, path: []const u8, id: ?[]const u8) Allocator.Error!void {
    if (v != .object) return;
    for (v.object) |m| {
        if (schema.hasField(o, m.key)) continue;
        const kpath = try std.fmt.allocPrint(a, "{s}/{s}", .{ path, m.key });
        if (suggestKey(a, o, m.key)) |sg| {
            const valid_target = schema.hasField(o, sg.to);
            if (sg.hint.len > 0) {
                diags.addFix(.warning, "W_UNKNOWN_KEY", id, kpath, "unknown key \"{s}\" in {s} ({s}): it is ignored (kept in the file). {s}", .{ m.key, path, o.name, sg.hint }, try std.fmt.allocPrint(a, "remove \"{s}\" or follow the hint above (`kerf schema {s}`)", .{ m.key, o.name }));
            } else if (valid_target) {
                diags.addFix(.warning, "W_UNKNOWN_KEY", id, kpath, "unknown key \"{s}\" in {s} ({s}): it is ignored (kept in the file). Did you mean \"{s}\"?", .{ m.key, path, o.name, sg.to }, try std.fmt.allocPrint(a, "rename \"{s}\" to \"{s}\" (`kerf schema {s}`)", .{ m.key, sg.to, o.name }));
            } else {
                diags.addFix(.warning, "W_UNKNOWN_KEY", id, kpath, "unknown key \"{s}\" in {s} ({s}): it is ignored (kept in the file). Valid keys: {s}", .{ m.key, path, o.name, keyList(a, o) }, try std.fmt.allocPrint(a, "remove \"{s}\" (`kerf schema {s}`)", .{ m.key, o.name }));
            }
        } else {
            diags.addFix(.warning, "W_UNKNOWN_KEY", id, kpath, "unknown key \"{s}\" in {s} ({s}): it is ignored (kept in the file). Valid keys: {s}", .{ m.key, path, o.name, keyList(a, o) }, try std.fmt.allocPrint(a, "remove \"{s}\" or use one of the valid keys (`kerf schema {s}`)", .{ m.key, o.name }));
        }
    }
}

fn strOf(v: json.Value, key: []const u8) []const u8 {
    return if (v.get(key)) |x| (x.str() orelse "") else "";
}

fn unknownKeys(a: Allocator, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    try lintObject(a, diags, doc, &schema.doc, "doc", null);
    if (doc.get("components")) |cs| if (cs.arr()) |ca| for (ca) |c| {
        if (c != .object) continue;
        const cid = strOf(c, "id");
        if (cid.len == 0) continue;
        const base = try std.fmt.allocPrint(a, "components/{s}", .{cid});
        if (c.get("at")) |at| try lintObject(a, diags, at, &schema.at, try std.fmt.allocPrint(a, "{s}/at", .{base}), cid);
        if (c.get("array")) |ar| try lintObject(a, diags, ar, &schema.array, try std.fmt.allocPrint(a, "{s}/array", .{base}), cid);
        if (c.get("acknowledge")) |ak| if (ak.arr()) |items| for (items, 0..) |it, i| {
            try lintObject(a, diags, it, &schema.ack, try std.fmt.allocPrint(a, "{s}/acknowledge/{d}", .{ base, i }), cid);
        };
    };
    const vs = (doc.get("views") orelse return).arr() orelse return;
    for (vs) |v| {
        if (v != .object) continue;
        const vid = strOf(v, "id");
        if (vid.len == 0) continue;
        const vbase = try std.fmt.allocPrint(a, "views/{s}", .{vid});
        try lintObject(a, diags, v, &schema.view, vbase, vid);
        const anns = (v.get("annotations") orelse continue).arr() orelse continue;
        for (anns) |an| {
            if (an != .object) continue;
            const aid = strOf(an, "id");
            if (aid.len == 0) continue;
            const ty = strOf(an, "type");
            const o: *const schema.Object = if (std.mem.eql(u8, ty, "dim")) &schema.dim else if (std.mem.eql(u8, ty, "label")) &schema.label else &schema.note;
            const apath = try std.fmt.allocPrint(a, "{s}/annotations/{s}", .{ vbase, aid });
            try lintObject(a, diags, an, o, apath, aid);
            if (an.get("cite")) |cv| if (cv.arr()) |items| for (items, 0..) |ci, i| {
                try lintObject(a, diags, ci, &schema.cite, try std.fmt.allocPrint(a, "{s}/cite/{d}", .{ apath, i }), aid);
            };
        }
    }
}

// ---- W_NOTE_STYLE ------------------------------------------------------------------------------------------------------------

const Abbr = struct { word: []const u8, abbr: []const u8 };

/// Spelled-out word(s) that the house style abbreviates (matched as whole words, case-insensitive).
const spelled_out = [_]Abbr{
    .{ .word = "GYPSUM BOARD", .abbr = "GYP. BD." },
    .{ .word = "PRESSURE TREATED", .abbr = "PT" },
    .{ .word = "ON CENTER", .abbr = "O.C." },
    .{ .word = "CONCRETE", .abbr = "CONC." },
    .{ .word = "CONTINUOUS", .abbr = "CONT." },
    .{ .word = "EACH", .abbr = "EA." },
    .{ .word = "BOTTOM", .abbr = "BOTT." },
    .{ .word = "REINFORCING", .abbr = "REINF." },
    .{ .word = "MINIMUM", .abbr = "MIN." },
    .{ .word = "DIAMETER", .abbr = "DIA." },
};

/// Abbreviations that legitimately end a note with a period (no trailing-period warning).
const period_abbrs = [_][]const u8{ "MIN", "MAX", "EA", "CONT", "TYP", "DIA", "GA", "SIM", "BOTT", "CONC", "EMBED", "MFR", "FTG", "GRD", "HDR", "DBL", "STL", "GALV", "VERT", "HORIZ", "GYP", "BD", "REINF", "SHTG", "CLR", "PLY", "NO", "STD", "SQ", "EQ", "NOM", "APPROX", "REQD", "SPEC", "DWG", "DWGS", "EXIST", "THK", "ALT", "UNO" };

fn isWordChar(c: u8) bool {
    return std.ascii.isAlphabetic(c);
}

/// Index of the next whole-word, case-insensitive match of `phrase` in `text` at or after `from`.
fn findWord(text: []const u8, phrase: []const u8, from: usize) ?usize {
    var i = from;
    while (i + phrase.len <= text.len) : (i += 1) {
        if (!std.ascii.eqlIgnoreCase(text[i .. i + phrase.len], phrase)) continue;
        if (i > 0 and isWordChar(text[i - 1])) continue;
        if (i + phrase.len < text.len and isWordChar(text[i + phrase.len])) continue;
        return i;
    }
    return null;
}

pub const NoteStyle = struct {
    /// Human-readable list of the problems ("" = fine).
    issues: []const u8,
    /// The text with every problem fixed.
    fixed: []const u8,
};

fn hasDashFraction(text: []const u8) bool {
    var i: usize = 0;
    while (i + 3 < text.len) : (i += 1) {
        if (!std.ascii.isDigit(text[i]) or text[i + 1] != '-') continue;
        var j = i + 2;
        const s = j;
        while (j < text.len and std.ascii.isDigit(text[j])) j += 1;
        if (j > s and j < text.len and text[j] == '/') return true;
    }
    return false;
}

fn lastToken(text: []const u8) []const u8 {
    const t = std.mem.trimEnd(u8, text, " \t\r\n");
    const sp = std.mem.lastIndexOfAny(u8, t, " \t\r\n(") orelse return t;
    return t[sp + 1 ..];
}

/// Check one note text against the house style (SPEC 19). Pure; used by the lint and by tests.
pub fn checkNoteText(a: Allocator, text: []const u8) Allocator.Error!NoteStyle {
    var issues: std.ArrayList(u8) = .empty;
    var has_lower = false;
    for (text) |c| if (std.ascii.isLower(c)) {
        has_lower = true;
    };
    if (has_lower) try addIssue(a, &issues, "lowercase letters (notes are UPPERCASE)");
    // " x " as a size separator
    var x_sep = false;
    var i: usize = 0;
    while (i + 2 < text.len) : (i += 1) {
        if (text[i] == ' ' and text[i + 1] == 'x' and text[i + 2] == ' ') x_sep = true;
    }
    if (x_sep) try addIssue(a, &issues, "\" x \" as a size separator (use \" X \")");
    if (hasDashFraction(text)) try addIssue(a, &issues, "dashed fraction like 1-1/2\" (house style is 1 1/2\")");
    // spelled-out words
    var fixed: std.ArrayList(u8) = .empty;
    try fixed.appendSlice(a, text);
    for (spelled_out) |sp| {
        if (findWord(text, sp.word, 0) != null) {
            try addIssue(a, &issues, try std.fmt.allocPrint(a, "\"{s}\" spelled out (use \"{s}\")", .{ sp.word, sp.abbr }));
        }
    }
    // trailing period (abbreviations keep theirs)
    const trimmed = std.mem.trimEnd(u8, text, " \t\r\n");
    var trailing = false;
    if (trimmed.len > 1 and trimmed[trimmed.len - 1] == '.') {
        const tok = lastToken(trimmed);
        const base = std.mem.trimEnd(u8, tok, ".");
        var ok = std.mem.indexOfScalar(u8, base, '.') != null; // O.C. U.N.O. T.O.
        if (!ok) for (period_abbrs) |ab| if (std.ascii.eqlIgnoreCase(ab, base)) {
            ok = true;
        };
        if (!ok and base.len > 0 and !std.ascii.isAlphabetic(base[base.len - 1])) ok = true; // 6". 10'-0".
        if (!ok) trailing = true;
    }
    if (trailing) try addIssue(a, &issues, "trailing period (notes are not sentences)");
    if (issues.items.len == 0) return .{ .issues = "", .fixed = text };

    // build the corrected text
    var work: std.ArrayList(u8) = .empty;
    try work.appendSlice(a, text);
    // spelled-out replacement first (whole words, any case)
    for (spelled_out) |sp| {
        var guard: usize = 0;
        while (findWord(work.items, sp.word, 0)) |at| : (guard += 1) {
            if (guard > 50) break;
            const next = try std.mem.concat(a, u8, &.{ work.items[0..at], sp.abbr, work.items[at + sp.word.len ..] });
            work.clearRetainingCapacity();
            try work.appendSlice(a, next);
        }
    }
    const up = try std.ascii.allocUpperString(a, work.items);
    var out: std.ArrayList(u8) = .empty;
    var k: usize = 0;
    while (k < up.len) : (k += 1) {
        // 1-1/2 -> 1 1/2
        if (up[k] == '-' and k > 0 and k + 1 < up.len and std.ascii.isDigit(up[k - 1]) and std.ascii.isDigit(up[k + 1])) {
            var j = k + 1;
            while (j < up.len and std.ascii.isDigit(up[j])) j += 1;
            if (j < up.len and up[j] == '/') {
                try out.append(a, ' ');
                continue;
            }
        }
        try out.append(a, up[k]);
    }
    var res: []const u8 = std.mem.trimEnd(u8, out.items, " \t\r\n");
    if (trailing and res.len > 0 and res[res.len - 1] == '.') res = res[0 .. res.len - 1];
    return .{ .issues = issues.items, .fixed = res };
}

fn addIssue(a: Allocator, list: *std.ArrayList(u8), what: []const u8) Allocator.Error!void {
    if (list.items.len > 0) try list.appendSlice(a, "; ");
    try list.appendSlice(a, what);
}

fn noteStyle(a: Allocator, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    const vs = (doc.get("views") orelse return).arr() orelse return;
    for (vs) |v| {
        const vid = strOf(v, "id");
        const anns = (v.get("annotations") orelse continue).arr() orelse continue;
        for (anns) |an| {
            if (!std.mem.eql(u8, strOf(an, "type"), "note")) continue;
            const text = (if (an.get("text")) |t| t.str() else null) orelse continue;
            const aid = strOf(an, "id");
            const st = try checkNoteText(a, text);
            if (st.issues.len == 0) continue;
            diags.addFix(.warning, "W_NOTE_STYLE", aid, try std.fmt.allocPrint(a, "views/{s}/annotations/{s}/text", .{ vid, aid }), "note '{s}' in view {s} breaks the house note style: {s}. Text: \"{s}\"", .{ aid, vid, st.issues, text }, try std.fmt.allocPrint(a, "set text to \"{s}\"", .{st.fixed}));
        }
    }
}

// ---- meta.requested -> W_REQUESTED_MISSING ----------------------------------------------------------------------------------

/// `meta.requested` must be an array of non-empty strings; every item that matches nothing in the document warns.
fn requestedMissing(a: Allocator, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    const meta = doc.get("meta") orelse return;
    const rv = meta.get("requested") orelse return;
    const shape_fix = "e.g. \"meta\": {\"requested\": [\"cmu wall\", \"bond beam\", \"H2.5A ties\"]} (the designer's asks, one short phrase each)";
    const items = rv.arr() orelse {
        if (!rv.isNull()) diags.addFix(.@"error", "E_PARAM", null, "meta/requested", "'meta.requested' must be an array of strings, got a {s}", .{rv.kindName()}, shape_fix);
        return;
    };
    var bad = false;
    for (items, 0..) |it, i| {
        const s = std.mem.trim(u8, it.str() orelse "", " \t\r\n");
        if (s.len == 0) {
            bad = true;
            diags.addFix(.@"error", "E_PARAM", null, try std.fmt.allocPrint(a, "meta/requested/{d}", .{i}), "'meta.requested' entry {d} must be a non-empty string", .{i}, shape_fix);
        }
    }
    if (bad) return;
    for (try coverage.compute(a, doc), 0..) |item, i| {
        if (item.found.len > 0) continue;
        diags.addFix(.warning, "W_REQUESTED_MISSING", null, try std.fmt.allocPrint(a, "meta/requested/{d}", .{i}), "requested element \"{s}\" (meta.requested) matches no component id, type, label, model or note text", .{item.text}, try std.fmt.allocPrint(a, "build it (name the component after it, or add a note whose text says \"{s}\"), or remove it from meta.requested if the designer dropped it", .{item.text}));
    }
}

// ---- dims: default dir, W_DIM_ZERO ----------------------------------------------------------------------------------------------

/// Resolve dim endpoints without reporting reference errors (the view build reports those).
fn quietPoint(scene: *Scene, v: json.Value) ?geom.V2 {
    var scratch = model.Diags.init(scene.a);
    const saved = scene.diags;
    scene.diags = &scratch;
    defer scene.diags = saved;
    return scene.point(v, "", "");
}

/// Default `dir`: the dominant axis between the points (|dx| >= |dy| gives h, else v).
pub fn dominantDir(from: geom.V2, to: geom.V2) []const u8 {
    return if (@abs(to.x - from.x) >= @abs(to.y - from.y)) "h" else "v";
}

fn dimMeasure(from: geom.V2, to: geom.V2, dir: []const u8) f64 {
    if (std.mem.eql(u8, dir, "v")) return @abs(to.y - from.y);
    if (std.mem.eql(u8, dir, "aligned")) return from.dist(to);
    return @abs(to.x - from.x);
}

fn dimZero(a: Allocator, scene: *Scene, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    const vs = (doc.get("views") orelse return).arr() orelse return;
    for (vs) |v| {
        if (std.mem.eql(u8, strOf(v, "kind"), "iso")) continue;
        const vid = strOf(v, "id");
        const anns = (v.get("annotations") orelse continue).arr() orelse continue;
        for (anns) |an| {
            if (!std.mem.eql(u8, strOf(an, "type"), "dim")) continue;
            const fv = an.get("from") orelse continue;
            const tv = an.get("to") orelse continue;
            const from = quietPoint(scene, fv) orelse continue;
            const to = quietPoint(scene, tv) orelse continue;
            const explicit = strOf(an, "dir");
            const dir: []const u8 = if (explicit.len > 0) explicit else dominantDir(from, to);
            if (!(std.mem.eql(u8, dir, "h") or std.mem.eql(u8, dir, "v") or std.mem.eql(u8, dir, "aligned"))) continue;
            const m = dimMeasure(from, to, dir);
            if (m >= 1.0 / 16.0) continue;
            const aid = strOf(an, "id");
            const dx = @abs(to.x - from.x);
            const dy = @abs(to.y - from.y);
            const fmt = struct {
                fn f(al: Allocator, x: f64) []const u8 {
                    return units.fmtFtIn(al, x) catch "?";
                }
            }.f;
            const path = try std.fmt.allocPrint(a, "views/{s}/annotations/{s}", .{ vid, aid });
            if (dx < 1.0 / 16.0 and dy < 1.0 / 16.0) {
                diags.addFix(.warning, "W_DIM_ZERO", aid, path, "dim '{s}' in view {s} measures {s}: 'from' and 'to' are the same point", .{ aid, vid, fmt(a, m) }, "point `from` and `to` at two different anchors, e.g. two corners of the member");
            } else {
                const other: []const u8 = if (std.mem.eql(u8, dir, "h")) "v" else "h";
                diags.addFix(.warning, "W_DIM_ZERO", aid, path, "dim '{s}' in view {s} measures {s} along dir \"{s}\" (the points are {s} apart horizontally and {s} vertically)", .{ aid, vid, fmt(a, m), dir, fmt(a, dx), fmt(a, dy) }, try std.fmt.allocPrint(a, "set \"dir\": \"{s}\" (or \"aligned\" for the true distance)", .{other}));
            }
        }
    }
}

fn hasUndirectedDim(doc: json.Value) bool {
    const vs = (doc.get("views") orelse return false).arr() orelse return false;
    for (vs) |v| {
        const anns = (v.get("annotations") orelse continue).arr() orelse continue;
        for (anns) |an| {
            if (std.mem.eql(u8, strOf(an, "type"), "dim") and (an.get("dir") == null or an.get("dir").?.isNull())) return true;
        }
    }
    return false;
}

/// True when some dim has no `dir` (the document needs `withDimDirs` before it is drawn).
pub fn needsDimDirs(doc: json.Value) bool {
    return doc == .object and hasUndirectedDim(doc);
}

fn setMember(a: Allocator, obj: json.Value, key: []const u8, val: json.Value) Allocator.Error!json.Value {
    var ms: std.ArrayList(json.Member) = .empty;
    var done = false;
    for (obj.object) |m| {
        if (std.mem.eql(u8, m.key, key)) {
            try ms.append(a, .{ .key = key, .value = val });
            done = true;
        } else try ms.append(a, m);
    }
    if (!done) try ms.append(a, .{ .key = key, .value = val });
    return .{ .object = ms.items };
}

/// The document with `dir` filled in on every section-view dim that lacks one (dominant axis). The stored document is
/// never changed: renderers get this copy. Unresolvable endpoints are left alone (the view build reports them).
pub fn withDimDirs(a: Allocator, scene: *Scene, doc: json.Value) Allocator.Error!json.Value {
    if (!needsDimDirs(doc)) return doc;
    const vs = (doc.get("views") orelse return doc).arr() orelse return doc;
    const nvs = try a.alloc(json.Value, vs.len);
    for (vs, 0..) |v, vi| {
        nvs[vi] = v;
        if (v != .object) continue;
        const anns = (v.get("annotations") orelse continue).arr() orelse continue;
        var changed = false;
        const nan = try a.alloc(json.Value, anns.len);
        for (anns, 0..) |an, ai| {
            nan[ai] = an;
            if (an != .object or !std.mem.eql(u8, strOf(an, "type"), "dim")) continue;
            if (an.get("dir")) |d| if (!d.isNull()) continue;
            const from = quietPoint(scene, an.get("from") orelse continue) orelse continue;
            const to = quietPoint(scene, an.get("to") orelse continue) orelse continue;
            nan[ai] = try setMember(a, an, "dir", .{ .string = dominantDir(from, to) });
            changed = true;
        }
        if (changed) nvs[vi] = try setMember(a, v, "annotations", .{ .array = nan });
    }
    return setMember(a, doc, "views", .{ .array = nvs });
}

/// Compile-and-patch for callers that hold only the document (api export/drawing): one extra compile, only when a dim lacks `dir`.
pub fn withDimDirsFromDoc(a: Allocator, doc: json.Value, st: *const @import("style.zig").Style) Allocator.Error!json.Value {
    if (!needsDimDirs(doc)) return doc;
    var scratch = model.Diags.init(a);
    const scene = try @import("compile.zig").compile(a, doc, st, &scratch);
    return withDimDirs(a, scene, doc);
}

// ---- acknowledge ------------------------------------------------------------------------------------------------------------------

/// Warnings are suppressed; info codes (I_*) are accepted as a no-op (an agent acknowledging I_SOLID_USED is not an error).
fn isAckable(code: []const u8) bool {
    return std.mem.startsWith(u8, code, "W_") or std.mem.startsWith(u8, code, "I_");
}

/// Shape errors of `acknowledge` (array of {code, reason}; codes must be warning codes).
fn ackShape(a: Allocator, scene: *Scene, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    _ = scene;
    const cs = (doc.get("components") orelse return).arr() orelse return;
    for (cs) |c| {
        const ak = c.get("acknowledge") orelse continue;
        if (ak.isNull()) continue;
        const cid = strOf(c, "id");
        const path = try std.fmt.allocPrint(a, "components/{s}/acknowledge", .{cid});
        const items = ak.arr() orelse {
            diags.addFix(.@"error", "E_PARAM", cid, path, "'acknowledge' must be an array of {{\"code\": \"W_...\", \"reason\": \"...\"}}", .{}, "e.g. \"acknowledge\": [{\"code\": \"W_UNTREATED_CONTACT\", \"reason\": \"truss seat moisture barrier by mfr.\"}]");
            continue;
        };
        for (items, 0..) |it, i| {
            const code = strOf(it, "code");
            const reason = std.mem.trim(u8, strOf(it, "reason"), " ");
            const ipath = try std.fmt.allocPrint(a, "{s}/{d}", .{ path, i });
            if (it != .object or code.len == 0 or reason.len == 0) {
                diags.addFix(.@"error", "E_PARAM", cid, ipath, "acknowledge entry {d} of '{s}' needs a string \"code\" and a non-empty \"reason\"", .{ i, cid }, "e.g. {\"code\": \"W_UNTREATED_CONTACT\", \"reason\": \"why this is fine\"}");
            } else if (!isAckable(code)) {
                diags.addFix(.@"error", "E_PARAM", cid, ipath, "'{s}' cannot be acknowledged: only warnings (W_*) can (I_* is accepted and ignored); errors must be fixed", .{code}, "fix the cause, or acknowledge a W_ code");
            }
        }
    }
}

fn mentions(d: model.Diag, id: []const u8) bool {
    if (d.id) |x| if (std.mem.eql(u8, x, id)) return true;
    // pair diagnostics (W_OVERLAP, W_NEAR_MISS, W_UNTREATED_CONTACT) name both members in quotes
    var buf: [128]u8 = undefined;
    const needle = std.fmt.bufPrint(&buf, "'{s}'", .{id}) catch return false;
    if (std.mem.eql(u8, d.code, "W_OVERLAP") or std.mem.eql(u8, d.code, "W_NEAR_MISS") or std.mem.eql(u8, d.code, "W_UNTREATED_CONTACT")) {
        return std.mem.indexOf(u8, d.message, needle) != null;
    }
    return false;
}

/// Replace every warning that a component acknowledges with one `I_ACK` info line (code, component, reason).
/// Call after all other diagnostics are collected.
pub fn applyAcknowledge(a: Allocator, doc: json.Value, diags: *model.Diags) Allocator.Error!void {
    if (doc != .object) return;
    const cs = (doc.get("components") orelse return).arr() orelse return;
    var any = false;
    for (cs) |c| if (c.get("acknowledge") != null) {
        any = true;
    };
    if (!any) return;
    var kept: std.ArrayList(model.Diag) = .empty;
    var acks: std.ArrayList(model.Diag) = .empty;
    for (diags.list.items) |d| {
        var matched: ?struct { cid: []const u8, reason: []const u8 } = null;
        if (d.level == .warning) {
            outer: for (cs) |c| {
                const cid = strOf(c, "id");
                const ak = c.get("acknowledge") orelse continue;
                const items = ak.arr() orelse continue;
                for (items) |it| {
                    if (it != .object) continue;
                    if (!std.mem.eql(u8, strOf(it, "code"), d.code)) continue;
                    if (std.mem.trim(u8, strOf(it, "reason"), " ").len == 0) continue;
                    if (mentions(d, cid)) {
                        matched = .{ .cid = cid, .reason = strOf(it, "reason") };
                        break :outer;
                    }
                }
            }
        }
        if (matched) |m| {
            try acks.append(a, .{ .level = .info, .code = "I_ACK", .id = m.cid, .path = d.path, .message = try std.fmt.allocPrint(a, "{s} on '{s}' acknowledged: {s} (was: {s})", .{ d.code, m.cid, m.reason, d.message }) });
        } else try kept.append(a, d);
    }
    // one I_ACK per (code, component, message) in document order, after the remaining diagnostics
    try kept.appendSlice(a, acks.items);
    diags.list = kept;
}

// ---- tests ---------------------------------------------------------------------------------------------------------------------

test "note style: house style passes, drift is caught and fixed" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    const good = [_][]const u8{
        "2X6 PT SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.",
        "(2) #5 CONT. BOTT., 3\" CLR.",
        "SIMPSON H2.5A HURRICANE TIE @ EA. TRUSS, INSTALL PER MFR.",
        "2X6 P.T. SILL PLATE",
        "(2) 2X6 JACK STUDS U.N.O.",
        "FINISH GRADE, SLOPE AWAY 6\" MIN. IN FIRST 10'-0\"",
        "5 1/4\" X 11 7/8\" PSL BEAM, T.O. BEAM FLUSH W/ T.O. DBL. TOP PLATE @ 8'-1 1/8\"",
        "7\" MIN. EMBED.",
    };
    for (good) |t| try std.testing.expectEqualStrings("", (try checkNoteText(a, t)).issues);
    const r1 = try checkNoteText(a, "12\" x 18\" footing, 1-1/2\" deep.");
    try std.testing.expect(std.mem.indexOf(u8, r1.issues, "lowercase") != null);
    try std.testing.expect(std.mem.indexOf(u8, r1.issues, " x ") != null);
    try std.testing.expect(std.mem.indexOf(u8, r1.issues, "dashed fraction") != null);
    try std.testing.expect(std.mem.indexOf(u8, r1.issues, "trailing period") != null);
    try std.testing.expectEqualStrings("12\" X 18\" FOOTING, 1 1/2\" DEEP", r1.fixed);
    const r2 = try checkNoteText(a, "GYPSUM BOARD CEILING, EACH SIDE, 2 BOTTOM BARS CONTINUOUS");
    try std.testing.expect(std.mem.indexOf(u8, r2.issues, "GYP. BD.") != null);
    try std.testing.expectEqualStrings("GYP. BD. CEILING, EA. SIDE, 2 BOTT. BARS CONT.", r2.fixed);
    const r3 = try checkNoteText(a, "TRUSS BEARS DIRECTLY ON BOND BEAM.");
    try std.testing.expectEqualStrings("trailing period (notes are not sentences)", r3.issues);
    try std.testing.expectEqualStrings("TRUSS BEARS DIRECTLY ON BOND BEAM", r3.fixed);
    // spelled-out must be whole words
    try std.testing.expectEqualStrings("", (try checkNoteText(a, "CONCRETELY EACHOTHER")).issues);
}
