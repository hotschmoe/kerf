//! Folder-of-documents helpers shared by the CLI and `kerf serve`: the op log
//! (`<file>.log.jsonl`, spec/SERVE.md), atomic writes, new-document text and file name rules.
//!
//! Timestamps are the ONLY nondeterministic bytes in Kerf, and they live only in the log.

const std = @import("std");
const kerf = @import("kerf");
const Io = std.Io;
const Allocator = std.mem.Allocator;

pub const doc_suffix = ".kerf.json";
pub const log_suffix = ".log.jsonl";

/// `2026-10-06T01:23:45Z` (UTC, second resolution).
pub fn isoTime(io: Io, buf: *[20]u8) []const u8 {
    const now = Io.Timestamp.now(io, .real);
    const secs: u64 = @intCast(@max(0, @divTrunc(now.nanoseconds, std.time.ns_per_s)));
    return isoFromSeconds(secs, buf);
}

pub fn isoFromSeconds(secs: u64, buf: *[20]u8) []const u8 {
    const es = std.time.epoch.EpochSeconds{ .secs = secs };
    const ed = es.getEpochDay();
    const yd = ed.calculateYearDay();
    const md = yd.calculateMonthDay();
    const ds = es.getDaySeconds();
    return std.fmt.bufPrint(buf, "{d:0>4}-{d:0>2}-{d:0>2}T{d:0>2}:{d:0>2}:{d:0>2}Z", .{
        yd.year,
        md.month.numeric(),
        md.day_index + 1,
        ds.getHoursIntoDay(),
        ds.getMinutesIntoHour(),
        ds.getSecondsIntoMinute(),
    }) catch unreachable;
}

/// A document file name the server will touch: a plain `*.kerf.json` base name, no separators or dots-only tricks.
pub fn validDocFile(name: []const u8) bool {
    if (name.len <= doc_suffix.len or name.len > 200) return false;
    if (!std.mem.endsWith(u8, name, doc_suffix)) return false;
    if (name[0] == '.') return false;
    for (name) |c| switch (c) {
        'a'...'z', 'A'...'Z', '0'...'9', '.', '_', '-', ' ', '(', ')' => {},
        else => return false,
    };
    return true;
}

/// `<file>.log.jsonl` for a document path (the log sits next to the document).
pub fn logPath(a: Allocator, doc_path: []const u8) Allocator.Error![]u8 {
    return std.mem.concat(a, u8, &.{ doc_path, log_suffix });
}

/// Replace `path` atomically (temp file + rename) so readers never see a half-written document.
pub fn writeFileAtomic(io: Io, dir: Io.Dir, path: []const u8, bytes: []const u8) !void {
    var af = try dir.createFileAtomic(io, path, .{ .replace = true });
    defer af.deinit(io);
    try af.file.writeStreamingAll(io, bytes);
    try af.replace(io);
}

/// Append one line (a newline is added) to `path`, creating it when missing.
///
/// Windows rule (field-tested): never open append-only (FILE_APPEND_DATA without the other write bits) and never
/// request a write-only handle that is later asked for its size. `Io.Dir.createFile` with `.read = false` asks for
/// GENERIC_WRITE only, and `File.length` then runs NtQueryInformationFile(FileAllInformation), which needs
/// FILE_READ_ATTRIBUTES: AccessDenied. So: a normal read+write handle (create if missing, never truncate), find the end,
/// write there (positional write, which is "seek to end + write" without moving a shared cursor).
pub fn appendLine(io: Io, dir: Io.Dir, path: []const u8, line: []const u8) !void {
    var f = try dir.createFile(io, path, .{ .truncate = false, .read = true });
    defer f.close(io);
    const end = try f.length(io);
    var small: [4096]u8 = undefined;
    if (line.len < small.len) {
        @memcpy(small[0..line.len], line);
        small[line.len] = '\n';
        try f.writePositionalAll(io, small[0 .. line.len + 1], end); // one write: concurrent appenders cannot split a line
    } else {
        try f.writePositionalAll(io, line, end);
        try f.writePositionalAll(io, "\n", end + line.len);
    }
}

pub const LogMeta = struct {
    who: []const u8,
    tool: []const u8,
    why: []const u8 = "",
};

/// One compact JSON log line (no trailing newline). `ops` is JSON text (array, or the `{ops:[…]}` wrapper
/// the engine also accepts); `changed` are the ids the engine reported; `summary` is the engine summary text.
pub fn buildEntry(a: Allocator, io: Io, meta: LogMeta, ops_text: []const u8, changed: []const []const u8, summary: []const u8) ![]u8 {
    var tbuf: [20]u8 = undefined;
    const ts = isoTime(io, &tbuf);
    var perr: kerf.json.ParseError = undefined;
    var ops_v: kerf.json.Value = .{ .array = &.{} };
    if (try kerf.json.parse(a, ops_text, &perr)) |v| {
        ops_v = v;
        if (v == .object) if (v.get("ops")) |inner| {
            ops_v = inner;
        };
    }
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"ts\":");
    try kerf.json.writeString(&out, a, ts);
    try out.appendSlice(a, ",\"who\":");
    try kerf.json.writeString(&out, a, meta.who);
    try out.appendSlice(a, ",\"tool\":");
    try kerf.json.writeString(&out, a, meta.tool);
    try out.appendSlice(a, ",\"why\":");
    try kerf.json.writeString(&out, a, meta.why);
    try out.appendSlice(a, ",\"ops\":");
    try kerf.json.writeCompact(&out, a, ops_v);
    try out.appendSlice(a, ",\"changed\":[");
    for (changed, 0..) |c, i| {
        if (i > 0) try out.append(a, ',');
        try kerf.json.writeString(&out, a, c);
    }
    try out.appendSlice(a, "],\"summary_head\":");
    const head = if (std.mem.indexOfScalar(u8, summary, '\n')) |nl| summary[0..nl] else summary;
    try kerf.json.writeString(&out, a, std.mem.trimEnd(u8, head, "\r"));
    try out.append(a, '}');
    return out.items;
}

/// Canonical text of an empty document, like `kerf new` makes. `id_src` becomes the id (lowercased,
/// non-alphanumerics to `-`).
pub fn newDocText(a: Allocator, id_src: []const u8, title: []const u8) ![]u8 {
    var id: std.ArrayList(u8) = .empty;
    for (id_src) |c| try id.append(a, if (std.ascii.isAlphanumeric(c)) std.ascii.toLower(c) else '-');
    var doc: std.ArrayList(u8) = .empty;
    try doc.appendSlice(a, "{\"kerf\":\"0.1\",\"id\":");
    try kerf.json.writeString(&doc, a, id.items);
    try doc.appendSlice(a, ",\"title\":");
    try kerf.json.writeString(&doc, a, title);
    try doc.appendSlice(a, ",\"meta\":{\"jurisdiction\":{\"code\":\"IRC\",\"edition\":2021}},\"run\":[-24,24],\"components\":[],\"views\":[]}");
    var perr: kerf.json.ParseError = undefined;
    const v = (try kerf.json.parse(a, doc.items, &perr)) orelse return error.BadDoc;
    return kerf.canon.write(a, v);
}

/// Base name without `.kerf.json` (or without any extension for other names).
pub fn docStem(file: []const u8) []const u8 {
    const base = std.fs.path.basename(file);
    if (std.mem.endsWith(u8, base, doc_suffix)) return base[0 .. base.len - doc_suffix.len];
    return if (std.mem.indexOfScalar(u8, base, '.')) |dot| base[0..dot] else base;
}

/// Summary text of the document (`check`), or "" when the engine rejects it.
pub fn checkSummary(a: Allocator, doc_text: []const u8) ![]const u8 {
    const input = try std.fmt.allocPrint(a, "{{\"doc\":{s}}}", .{std.mem.trim(u8, doc_text, " \t\r\n")});
    const r = try kerf.call(a, "check", input);
    if (!r.ok) return "";
    var perr: kerf.json.ParseError = undefined;
    const v = (try kerf.json.parse(a, r.bytes, &perr)) orelse return "";
    return if (v.get("summary")) |s| (s.str() orelse "") else "";
}

test "appendLine creates, never truncates, appends after existing bytes" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    try appendLine(io, tmp.dir, "a.log.jsonl", "{\"n\":1}");
    try appendLine(io, tmp.dir, "a.log.jsonl", "{\"n\":2}");
    var big: [6000]u8 = undefined; // exercises the two-write path
    @memset(&big, 'x');
    try appendLine(io, tmp.dir, "a.log.jsonl", &big);
    const got = try tmp.dir.readFileAlloc(io, "a.log.jsonl", std.testing.allocator, .limited(1 << 20));
    defer std.testing.allocator.free(got);
    try std.testing.expectEqualStrings("{\"n\":1}\n{\"n\":2}\n", got[0..16]);
    try std.testing.expectEqual(@as(usize, 16 + 6001), got.len);
    try std.testing.expectEqual(@as(u8, '\n'), got[got.len - 1]);
}

test "isoFromSeconds" {
    var b: [20]u8 = undefined;
    try std.testing.expectEqualStrings("1970-01-01T00:00:00Z", isoFromSeconds(0, &b));
    try std.testing.expectEqualStrings("2026-10-06T01:23:45Z", isoFromSeconds(1791249825, &b));
}

test "validDocFile" {
    try std.testing.expect(validDocFile("truss-cmu.kerf.json"));
    try std.testing.expect(!validDocFile("../x.kerf.json"));
    try std.testing.expect(!validDocFile("a/b.kerf.json"));
    try std.testing.expect(!validDocFile("a\\b.kerf.json"));
    try std.testing.expect(!validDocFile(".kerf.json"));
    try std.testing.expect(!validDocFile("x.json"));
    try std.testing.expect(!validDocFile(".hidden.kerf.json"));
    try std.testing.expect(!validDocFile("x:y.kerf.json"));
}

test "buildEntry shape" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    const line = try buildEntry(a, io, .{ .who = "agent", .tool = "kerf-cli", .why = "Add \"x\"" }, "{\"ops\":[{\"op\":\"add\",\"path\":\"components\"}]}", &.{ "heta", "b" }, "DOC t 1 components\nmore");
    var perr: kerf.json.ParseError = undefined;
    const v = (try kerf.json.parse(a, line, &perr)).?;
    try std.testing.expectEqualStrings("agent", v.get("who").?.str().?);
    try std.testing.expectEqualStrings("Add \"x\"", v.get("why").?.str().?);
    try std.testing.expectEqual(@as(usize, 1), v.get("ops").?.arr().?.len);
    try std.testing.expectEqual(@as(usize, 2), v.get("changed").?.arr().?.len);
    try std.testing.expectEqualStrings("DOC t 1 components", v.get("summary_head").?.str().?);
    try std.testing.expect(std.mem.indexOfScalar(u8, line, '\n') == null);
}
