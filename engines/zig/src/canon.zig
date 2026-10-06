//! Canonical document JSON (SPEC 16): 2-space indent, schema key order, unknown keys alphabetical.

const std = @import("std");
const json = @import("json.zig");
const catalog = @import("catalog.zig");
const schema = @import("schema.zig");
const Allocator = std.mem.Allocator;

const root_keys = schema.keys(&schema.doc);
const meta_keys = [_][]const u8{ "author", "discipline", "classification", "jurisdiction", "requested", "tags", "forked_from", "sheet", "date" };
const class_keys = [_][]const u8{ "uniformat", "masterformat" };
const juris_keys = [_][]const u8{ "code", "edition" };
const common_keys = [_][]const u8{ "id", "type", "label", "material", "at", "rotate", "slope", "mirror", "z", "array", "embedded", "visible", "shown", "acknowledge" };
const at_keys = schema.keys(&schema.at);
const ack_keys = schema.keys(&schema.ack);
const ref_keys = [_][]const u8{ "ref", "offset" };
const array_keys = schema.keys(&schema.array);
const recess_keys = [_][]const u8{ "width", "depth", "from_edge" };
const cover_keys = [_][]const u8{ "bottom", "sides", "top", "parts" };
const place_keys = [_][]const u8{ "in", "face", "cover", "count", "side_cover", "axis", "station" };
const profile_keys = [_][]const u8{ "rect", "circle", "points" };
const view_keys = schema.keys(&schema.view);
const crop_keys = [_][]const u8{ "x", "y" };
const cite_keys = schema.keys(&schema.cite);
const note_keys = schema.keys(&schema.note);
const dim_keys = schema.keys(&schema.dim);
const label_keys = schema.keys(&schema.label);

fn eql(a: []const u8, b: []const u8) bool {
    return std.mem.eql(u8, a, b);
}

/// Canonical key order of a component: the common keys, then the type's own parameters (a name list built per call from
/// the writer's allocator: no shared scratch, no length limit).
fn componentKeys(ctx: *const json.KeyCtx) Allocator.Error![]const []const u8 {
    var ty: []const u8 = "";
    for (ctx.obj) |m| if (eql(m.key, "type")) {
        if (m.value == .string) ty = m.value.string;
    };
    var keys: std.ArrayList([]const u8) = .empty;
    try keys.appendSlice(ctx.a, &common_keys);
    if (catalog.find(ty)) |e| {
        for (e.params) |p| {
            var it = std.mem.splitSequence(u8, p.name, ", ");
            while (it.next()) |nm| try keys.append(ctx.a, nm);
        }
    }
    return keys.items;
}

pub fn order(ctx: *const json.KeyCtx) Allocator.Error![]const []const u8 {
    const p = ctx.path;
    if (p.len == 0) return &root_keys;
    const last = p[p.len - 1];
    if (p.len == 1) {
        if (eql(last, "meta")) return &meta_keys;
        return &.{};
    }
    if (p.len == 2 and eql(p[0], "meta")) {
        if (eql(last, "classification")) return &class_keys;
        if (eql(last, "jurisdiction")) return &juris_keys;
    }
    if (p.len == 2 and eql(p[0], "components") and eql(last, "[]")) return try componentKeys(ctx);
    if (p.len == 2 and eql(p[0], "views") and eql(last, "[]")) return &view_keys;
    if (p.len == 3 and eql(p[0], "components")) {
        if (eql(last, "at")) return &at_keys;
        if (eql(last, "array")) return &array_keys;
        if (eql(last, "recess")) return &recess_keys;
        if (eql(last, "cover")) return &cover_keys;
        if (eql(last, "place")) return &place_keys;
        if (eql(last, "profile")) return &profile_keys;
    }
    if (p.len == 4 and eql(p[0], "components") and eql(p[2], "acknowledge") and eql(last, "[]")) return &ack_keys;
    // {ref, offset} objects anywhere below components
    if (eql(p[0], "components") and p.len >= 3) {
        for (ctx.obj) |m| if (eql(m.key, "ref")) return &ref_keys;
        if (p.len == 4 and eql(p[2], "at") and eql(last, "to")) return &ref_keys;
    }
    if (eql(p[0], "views")) {
        if (p.len == 3 and eql(last, "crop")) return &crop_keys;
        if (p.len == 4 and eql(p[2], "annotations") and eql(last, "[]")) {
            var ty: []const u8 = "";
            for (ctx.obj) |m| if (eql(m.key, "type")) {
                if (m.value == .string) ty = m.value.string;
            };
            if (eql(ty, "dim")) return &dim_keys;
            if (eql(ty, "label")) return &label_keys;
            return &note_keys;
        }
        if (p.len == 5 and eql(p[2], "annotations") and eql(last, "cite")) return &.{};
        if (p.len == 6 and eql(p[4], "cite") and eql(last, "[]")) return &cite_keys;
        if (p.len >= 5 and eql(p[2], "annotations")) {
            for (ctx.obj) |m| if (eql(m.key, "ref")) return &ref_keys;
        }
    }
    return &.{};
}

/// Canonical text of a document (with trailing newline).
pub fn write(a: Allocator, doc: json.Value) Allocator.Error![]u8 {
    var out: std.ArrayList(u8) = .empty;
    var pw = json.Pretty{ .out = &out, .a = a, .order = order };
    try pw.write(doc, 0);
    try out.append(a, '\n');
    return out.items;
}

test "canonical common key order covers every catalog common field (no drift)" {
    for (catalog.common) |c| {
        var found = false;
        for (common_keys) |k| if (std.mem.eql(u8, k, c.name)) {
            found = true;
        };
        try std.testing.expect(found);
    }
}

fn fmtWorker(src: []const u8, want: []const u8, failures: *std.atomic.Value(u32)) void {
    var i: usize = 0;
    while (i < 40) : (i += 1) {
        const input = std.fmt.allocPrint(std.heap.page_allocator, "{{\"doc\":{s}}}", .{src}) catch return;
        defer std.heap.page_allocator.free(input);
        const r = @import("api.zig").call(std.heap.page_allocator, "fmt", input) catch {
            _ = failures.fetchAdd(1, .monotonic);
            return;
        };
        defer std.heap.page_allocator.free(r.bytes);
        if (!std.mem.eql(u8, r.bytes, want)) _ = failures.fetchAdd(1, .monotonic);
    }
}

test "fmt is safe to call from several threads at once (no shared key scratch, REVIEW SAF-7)" {
    const testdocs = @import("testdocs.zig");
    var wants: [3][]u8 = undefined;
    for (testdocs.all, 0..) |src, i| {
        const input = try std.fmt.allocPrint(std.testing.allocator, "{{\"doc\":{s}}}", .{src});
        defer std.testing.allocator.free(input);
        const r = try @import("api.zig").call(std.testing.allocator, "fmt", input);
        wants[i] = r.bytes;
    }
    defer for (wants) |w| std.testing.allocator.free(w);
    var failures = std.atomic.Value(u32).init(0);
    var threads: [6]std.Thread = undefined;
    for (&threads, 0..) |*t, i| t.* = try std.Thread.spawn(.{}, fmtWorker, .{ testdocs.all[i % 3], wants[i % 3], &failures });
    for (threads) |t| t.join();
    try std.testing.expectEqual(@as(u32, 0), failures.load(.monotonic));
}
