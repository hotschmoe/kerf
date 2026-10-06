//! Requested-elements coverage (SPEC 21): `meta.requested` lists what the designer asked for; each item is matched
//! against the document (component ids, types, labels, models, sizes, and note / label text). The result feeds the
//! `COVERAGE` block of the summary and `W_REQUESTED_MISSING`.

const std = @import("std");
const json = @import("json.zig");
const Allocator = std.mem.Allocator;

pub const Item = struct {
    text: []const u8,
    /// Component ids (or `note <id>` when a note has no resolvable target) that cover the item; empty = MISSING.
    found: []const []const u8,
};

/// Words that carry no identity ("CMU wall w/ bond beam").
const stop_words = [_][]const u8{ "w", "with", "and", "at", "of", "the", "a", "an", "to", "for", "in", "on", "per" };

/// Lowercased word tokens of `text` (split on anything but letters and digits; a plural `s` is dropped), without stop words.
fn tokens(a: Allocator, text: []const u8) Allocator.Error![]const []const u8 {
    var out: std.ArrayList([]const u8) = .empty;
    var i: usize = 0;
    while (i < text.len) {
        while (i < text.len and !std.ascii.isAlphanumeric(text[i])) i += 1;
        const start = i;
        while (i < text.len and std.ascii.isAlphanumeric(text[i])) i += 1;
        if (i == start) break;
        var w = try std.ascii.allocLowerString(a, text[start..i]);
        if (w.len > 3 and w[w.len - 1] == 's' and w[w.len - 2] != 's') w = w[0 .. w.len - 1];
        var stop = false;
        for (stop_words) |s| if (std.mem.eql(u8, s, w)) {
            stop = true;
        };
        if (!stop) try out.append(a, w);
    }
    return out.items;
}

/// True when every item token occurs among the candidate tokens.
fn covers(item: []const []const u8, cand: []const []const u8) bool {
    if (item.len == 0) return false;
    for (item) |t| {
        var hit = false;
        for (cand) |c| if (std.mem.eql(u8, c, t)) {
            hit = true;
        };
        if (!hit) return false;
    }
    return true;
}

fn strOf(v: json.Value, key: []const u8) []const u8 {
    return if (v.get(key)) |x| (x.str() orelse "") else "";
}

/// The component id a note or label points at (`target` "comp.part", or `at` "comp@anchor"), "" when it names none.
fn targetComp(v: json.Value) []const u8 {
    var t = strOf(v, "target");
    if (t.len == 0) t = strOf(v, "at");
    if (t.len == 0) return "";
    const end = std.mem.indexOfAny(u8, t, ".@#") orelse t.len;
    return t[0..end];
}

fn hasComponent(doc: json.Value, id: []const u8) bool {
    const cs = (doc.get("components") orelse return false).arr() orelse return false;
    for (cs) |c| if (std.mem.eql(u8, strOf(c, "id"), id)) return true;
    return false;
}

fn addFound(a: Allocator, list: *std.ArrayList([]const u8), id: []const u8) Allocator.Error!void {
    for (list.items) |x| if (std.mem.eql(u8, x, id)) return;
    try list.append(a, id);
}

/// The `meta.requested` entries (strings only), in document order; empty when there are none.
pub fn requestedList(doc: json.Value) []const json.Value {
    const meta = doc.get("meta") orelse return &.{};
    const r = meta.get("requested") orelse return &.{};
    return r.arr() orelse &.{};
}

/// Coverage of every non-empty string in `meta.requested`; empty when the document has none.
pub fn compute(a: Allocator, doc: json.Value) Allocator.Error![]const Item {
    var out: std.ArrayList(Item) = .empty;
    for (requestedList(doc)) |rv| {
        const text = std.mem.trim(u8, rv.str() orelse continue, " \t\r\n");
        if (text.len == 0) continue;
        const want = try tokens(a, text);
        var found: std.ArrayList([]const u8) = .empty;
        if (doc.get("components")) |cs| if (cs.arr()) |ca| for (ca) |c| {
            const joined = try std.fmt.allocPrint(a, "{s} {s} {s} {s} {s}", .{ strOf(c, "id"), strOf(c, "type"), strOf(c, "label"), strOf(c, "model"), strOf(c, "size") });
            if (covers(want, try tokens(a, joined))) try addFound(a, &found, strOf(c, "id"));
        };
        if (doc.get("views")) |vs| if (vs.arr()) |va| for (va) |v| {
            const anns = (v.get("annotations") orelse continue).arr() orelse continue;
            for (anns) |an| {
                const ty = strOf(an, "type");
                if (!(std.mem.eql(u8, ty, "note") or std.mem.eql(u8, ty, "label"))) continue;
                if (!covers(want, try tokens(a, strOf(an, "text")))) continue;
                const tc = targetComp(an);
                if (tc.len > 0 and hasComponent(doc, tc)) {
                    try addFound(a, &found, tc);
                } else try addFound(a, &found, try std.fmt.allocPrint(a, "note {s}", .{strOf(an, "id")}));
            }
        };
        try out.append(a, .{ .text = text, .found = found.items });
    }
    return out.items;
}
