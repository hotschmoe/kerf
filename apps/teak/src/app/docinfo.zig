//! Read-only digest of a Kerf document for the UI (tables, tabs, notes).
//!
//! The document JSON stays the source of truth (owned by `Session`); this
//! module only extracts what the inspector and tab bar need. Everything is
//! allocated in one arena so a refresh is a single `deinit`.

const std = @import("std");
const Allocator = std.mem.Allocator;
const Value = std.json.Value;

pub const Cite = struct {
    code: []const u8 = "",
    edition: ?i64 = null,
    section: []const u8 = "",
    title: []const u8 = "",
    verified: bool = false,
};

pub const Note = struct {
    id: []const u8,
    kind: []const u8, // note | dim | label
    text: []const u8 = "",
    target: []const u8 = "",
    cites: []const Cite = &.{},
    place: ?[2]f64 = null,
};

pub const View = struct {
    id: []const u8,
    kind: []const u8 = "section",
    number: []const u8 = "",
    title: []const u8 = "",
    scale: []const u8 = "",
    notes: []const Note = &.{},
};

pub const Component = struct {
    id: []const u8,
    type: []const u8 = "",
    label: []const u8 = "",
};

pub const DocInfo = struct {
    arena: std.heap.ArenaAllocator,
    id: []const u8 = "",
    title: []const u8 = "",
    jurisdiction: []const u8 = "",
    components: []const Component = &.{},
    views: []const View = &.{},

    pub fn deinit(self: *DocInfo) void {
        self.arena.deinit();
    }

    pub fn viewIndex(self: *const DocInfo, id: []const u8) ?usize {
        for (self.views, 0..) |v, i| if (std.mem.eql(u8, v.id, id)) return i;
        return null;
    }

    pub fn countUnverified(self: *const DocInfo) usize {
        var n: usize = 0;
        for (self.views) |v| for (v.notes) |nt| for (nt.cites) |c| {
            if (!c.verified) n += 1;
        };
        return n;
    }
};

fn str(v: ?Value) []const u8 {
    const val = v orelse return "";
    return switch (val) {
        .string => |s| s,
        else => "",
    };
}

fn obj(v: ?Value) ?std.json.ObjectMap {
    const val = v orelse return null;
    return switch (val) {
        .object => |o| o,
        else => null,
    };
}

fn arr(v: ?Value) []const Value {
    const val = v orelse return &.{};
    return switch (val) {
        .array => |a| a.items,
        else => &.{},
    };
}

fn num(v: ?Value) ?f64 {
    const val = v orelse return null;
    return switch (val) {
        .integer => |i| @floatFromInt(i),
        .float => |f| f,
        else => null,
    };
}

pub fn parse(gpa: Allocator, doc_json: []const u8) !DocInfo {
    var info: DocInfo = .{ .arena = .init(gpa) };
    errdefer info.arena.deinit();
    const a = info.arena.allocator();

    const root = try std.json.parseFromSliceLeaky(Value, a, doc_json, .{});
    const o = obj(root) orelse return error.NotAnObject;

    info.id = str(o.get("id"));
    info.title = str(o.get("title"));
    if (obj(o.get("meta"))) |meta| {
        if (obj(meta.get("jurisdiction"))) |j| {
            const ed: i64 = if (num(j.get("edition"))) |e| @intFromFloat(e) else 0;
            info.jurisdiction = try std.fmt.allocPrint(a, "{s} {d}", .{ str(j.get("code")), ed });
        }
    }

    var comps: std.ArrayList(Component) = .empty;
    for (arr(o.get("components"))) |cv| {
        const co = obj(cv) orelse continue;
        try comps.append(a, .{ .id = str(co.get("id")), .type = str(co.get("type")), .label = str(co.get("label")) });
    }
    info.components = comps.items;

    var views: std.ArrayList(View) = .empty;
    for (arr(o.get("views"))) |vv| {
        const vo = obj(vv) orelse continue;
        var notes: std.ArrayList(Note) = .empty;
        for (arr(vo.get("annotations"))) |nv| {
            const no = obj(nv) orelse continue;
            var cites: std.ArrayList(Cite) = .empty;
            for (arr(no.get("cite"))) |cv| {
                const co = obj(cv) orelse continue;
                try cites.append(a, .{
                    .code = str(co.get("code")),
                    .edition = if (num(co.get("edition"))) |e| @as(i64, @intFromFloat(e)) else null,
                    .section = str(co.get("section")),
                    .title = str(co.get("title")),
                    .verified = std.mem.eql(u8, str(co.get("status")), "verified"),
                });
            }
            var place: ?[2]f64 = null;
            const pl = arr(no.get("place"));
            if (pl.len == 2) {
                if (num(pl[0])) |x| if (num(pl[1])) |y| {
                    place = .{ x, y };
                };
            }
            try notes.append(a, .{
                .id = str(no.get("id")),
                .kind = str(no.get("type")),
                .text = str(no.get("text")),
                .target = str(no.get("target")),
                .cites = cites.items,
                .place = place,
            });
        }
        try views.append(a, .{
            .id = str(vo.get("id")),
            .kind = str(vo.get("kind")),
            .number = str(vo.get("number")),
            .title = str(vo.get("title")),
            .scale = str(vo.get("scale")),
            .notes = notes.items,
        });
    }
    info.views = views.items;
    return info;
}

test "parse reference truss doc" {
    var info = try parse(std.testing.allocator, @embedFile("sample_truss_json"));
    defer info.deinit();
    try std.testing.expectEqualStrings("truss-bearing-cmu", info.id);
    try std.testing.expect(info.components.len > 5);
    try std.testing.expect(info.views.len >= 1);
    try std.testing.expect(info.views[0].notes.len >= 6);
    try std.testing.expect(info.countUnverified() > 0);
}
