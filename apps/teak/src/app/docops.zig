//! Builders for the designer's edit ops (SPEC §14). Each returns an owned JSON
//! array text ready for `Session.apply(..., .designer, ...)`.

const std = @import("std");
const Allocator = std.mem.Allocator;
const docinfo = @import("docinfo.zig");

fn str(w: *std.Io.Writer, s: []const u8) !void {
    try std.json.Stringify.encodeJsonString(s, .{}, w);
}

fn begin(w: *std.Io.Writer, view: []const u8, note: []const u8) !void {
    try w.writeAll("[{\"op\":\"update\",\"path\":");
    var buf: [256]u8 = undefined;
    const path = std.fmt.bufPrint(&buf, "views/{s}/annotations/{s}", .{ view, note }) catch return error.NoSpaceLeft;
    try str(w, path);
    try w.writeAll(",\"value\":{");
}

fn end(w: *std.Io.Writer) !void {
    try w.writeAll("}}]");
}

pub fn noteText(gpa: Allocator, view: []const u8, note: []const u8, text: []const u8) ![]u8 {
    var out: std.Io.Writer.Allocating = .init(gpa);
    errdefer out.deinit();
    const w = &out.writer;
    try begin(w, view, note);
    try w.writeAll("\"text\":");
    try str(w, text);
    try end(w);
    return out.toOwnedSlice();
}

pub fn notePlace(gpa: Allocator, view: []const u8, note: []const u8, x: f64, y: f64) ![]u8 {
    var out: std.Io.Writer.Allocating = .init(gpa);
    errdefer out.deinit();
    const w = &out.writer;
    try begin(w, view, note);
    try w.print("\"place\":[{d:.4},{d:.4}]", .{ x, y });
    try end(w);
    return out.toOwnedSlice();
}

/// Replace the whole `cite` array of a note. `override` replaces entry
/// `index` (used by verify toggles and edits); `append` adds one at the end;
/// `remove` drops one.
pub const CiteEdit = union(enum) {
    set_verified: struct { index: usize, verified: bool },
    append: docinfo.Cite,
    remove: usize,
    replace: struct { index: usize, cite: docinfo.Cite },
};

fn writeCite(w: *std.Io.Writer, c: docinfo.Cite, verified: bool) !void {
    try w.writeAll("{\"code\":");
    try str(w, c.code);
    if (c.edition) |e| try w.print(",\"edition\":{d}", .{e});
    try w.writeAll(",\"section\":");
    try str(w, c.section);
    if (c.title.len > 0) {
        try w.writeAll(",\"title\":");
        try str(w, c.title);
    }
    try w.print(",\"status\":\"{s}\"}}", .{if (verified) "verified" else "suggested"});
}

pub fn noteCites(gpa: Allocator, view: []const u8, note: docinfo.Note, edit: CiteEdit) ![]u8 {
    var out: std.Io.Writer.Allocating = .init(gpa);
    errdefer out.deinit();
    const w = &out.writer;
    try begin(w, view, note.id);
    try w.writeAll("\"cite\":[");
    var first = true;
    for (note.cites, 0..) |c, i| {
        var cc = c;
        var verified = c.verified;
        switch (edit) {
            .remove => |r| if (r == i) continue,
            .set_verified => |s| if (s.index == i) {
                verified = s.verified;
            },
            .replace => |r| if (r.index == i) {
                cc = r.cite;
                verified = false; // a changed citation must be re-verified
            },
            .append => {},
        }
        if (!first) try w.writeByte(',');
        first = false;
        try writeCite(w, cc, verified);
    }
    switch (edit) {
        .append => |c| {
            if (!first) try w.writeByte(',');
            try writeCite(w, c, false);
        },
        else => {},
    }
    try w.writeByte(']');
    try end(w);
    return out.toOwnedSlice();
}

test "note ops are valid JSON" {
    const a = std.testing.allocator;
    const t = try noteText(a, "A", "n1", "2X8 PT SILL \"X\"");
    defer a.free(t);
    var p = try std.json.parseFromSlice(std.json.Value, a, t, .{});
    defer p.deinit();
    try std.testing.expectEqualStrings("update", p.value.array.items[0].object.get("op").?.string);

    const note: docinfo.Note = .{ .id = "n1", .kind = "note", .cites = &.{
        .{ .code = "IRC", .edition = 2021, .section = "R403.1.6", .verified = false },
    } };
    const c = try noteCites(a, "A", note, .{ .set_verified = .{ .index = 0, .verified = true } });
    defer a.free(c);
    try std.testing.expect(std.mem.indexOf(u8, c, "\"status\":\"verified\"") != null);
    const c2 = try noteCites(a, "A", note, .{ .append = .{ .code = "IBC", .section = "1604", .edition = 2021 } });
    defer a.free(c2);
    var p2 = try std.json.parseFromSlice(std.json.Value, a, c2, .{});
    defer p2.deinit();
    try std.testing.expectEqual(@as(usize, 2), p2.value.array.items[0].object.get("value").?.object.get("cite").?.array.items.len);
}
