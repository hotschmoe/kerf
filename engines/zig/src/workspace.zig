//! Folder-of-documents helpers shared by the CLI and `kerf serve`: the op log
//! (`<file>.log.jsonl`, spec/SERVE.md), atomic writes, new-document text and file name rules.
//!
//! Timestamps are the ONLY nondeterministic bytes in Kerf, and they live only in the log.

const std = @import("std");
const builtin = @import("builtin");
const kerf = @import("kerf");
const fsx = @import("fsx.zig");
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

/// Names Windows reserves as devices, also with an extension (`CON.kerf.json` opens the console).
fn isWindowsDeviceName(name: []const u8) bool {
    const stem = if (std.mem.indexOfScalar(u8, name, '.')) |dot| name[0..dot] else name;
    const reserved = [_][]const u8{ "CON", "PRN", "AUX", "NUL" };
    for (reserved) |r| if (std.ascii.eqlIgnoreCase(stem, r)) return true;
    if (stem.len == 4 and (std.ascii.startsWithIgnoreCase(stem, "COM") or std.ascii.startsWithIgnoreCase(stem, "LPT")) and stem[3] >= '1' and stem[3] <= '9') return true;
    return false;
}

/// A document file name the server will touch: a plain `*.kerf.json` base name, no separators or dots-only tricks,
/// and nothing Windows would treat specially (device names, a stem ending in a dot or space, `:` streams).
pub fn validDocFile(name: []const u8) bool {
    if (name.len <= doc_suffix.len or name.len > 200) return false;
    if (!std.mem.endsWith(u8, name, doc_suffix)) return false;
    if (name[0] == '.' or name[0] == ' ') return false;
    const stem_last = name[name.len - doc_suffix.len - 1];
    if (stem_last == '.' or stem_last == ' ') return false;
    for (name) |c| switch (c) {
        'a'...'z', 'A'...'Z', '0'...'9', '.', '_', '-', ' ', '(', ')' => {},
        else => return false,
    };
    if (isWindowsDeviceName(name)) return false;
    return true;
}

/// `<file>.log.jsonl` for a document path (the log sits next to the document).
pub fn logPath(a: Allocator, doc_path: []const u8) Allocator.Error![]u8 {
    return std.mem.concat(a, u8, &.{ doc_path, log_suffix });
}

/// Best-effort `fsync` of the directory that contains `path` (relative to `dir`), so a rename into it survives a crash.
/// Not available on Windows.
pub fn syncDirOf(io: Io, dir: Io.Dir, path: []const u8) void {
    if (builtin.os.tag == .windows or builtin.os.tag == .wasi) return;
    const sub = std.fs.path.dirname(path) orelse ".";
    var d = dir.openDir(io, sub, .{ .iterate = true }) catch return; // "." also resolves the cwd handle; without `iterate` std opens O_PATH, which cannot be fsynced
    defer d.close(io);
    const f: Io.File = .{ .handle = d.handle, .flags = .{ .nonblocking = false } };
    f.sync(io) catch {};
}

/// Replace `path` atomically (temp file + rename) so readers never see a half-written document. The data is
/// `fsync`ed before the rename and the directory after it: after a crash or power loss the old or the new document
/// exists, never an empty one.
pub fn writeFileAtomic(io: Io, dir: Io.Dir, path: []const u8, bytes: []const u8) !void {
    var af = try dir.createFileAtomic(io, path, .{ .replace = true });
    defer af.deinit(io);
    try af.file.writeStreamingAll(io, bytes);
    try af.file.sync(io);
    try af.replace(io);
    syncDirOf(io, dir, path);
}

pub const AppendError = error{SymLink};

/// A plain file or nothing: a planted symlink at `path` must not turn a log append into a write elsewhere.
fn refuseSymlink(io: Io, dir: Io.Dir, path: []const u8) AppendError!void {
    const st = dir.statFile(io, path, .{ .follow_symlinks = false }) catch return; // missing is fine: it will be created
    if (st.kind == .sym_link) return error.SymLink;
}

/// Exclusive advisory lock (`flock`) on `<doc>.log.jsonl`, held across a whole read-modify-write of one document so
/// that two processes (the server, several `kerf apply -w`) cannot interleave and lose each other's edit, and through
/// which the log line is appended. On Windows it is a byte-range lock far past the end of the file
/// (`fsx.lockExclusive`), so readers of the log are never blocked. Best effort when the file system has no locks.
pub const DocLock = struct {
    file: Io.File,

    pub fn acquire(io: Io, dir: Io.Dir, log_path: []const u8) !DocLock {
        try refuseSymlink(io, dir, log_path);
        const f = try dir.createFile(io, log_path, .{ .truncate = false, .read = true });
        errdefer f.close(io);
        try fsx.lockExclusive(io, f);
        return .{ .file = f };
    }

    pub fn release(l: DocLock, io: Io) void {
        l.file.close(io); // closing releases the lock
    }

    /// Append one line (a newline is added) through the locked handle and `fsync` it.
    pub fn appendLine(l: DocLock, io: Io, line: []const u8) !void {
        try appendTo(io, l.file, line);
    }
};

fn appendTo(io: Io, f: Io.File, line: []const u8) !void {
    // Windows rule (field-tested): never open append-only (FILE_APPEND_DATA without the other write bits) and never
    // request a write-only handle that is later asked for its size. `Io.Dir.createFile` with `.read = false` asks for
    // GENERIC_WRITE only, and `File.length` then runs NtQueryInformationFile(FileAllInformation), which needs
    // FILE_READ_ATTRIBUTES: AccessDenied. So: a normal read+write handle (create if missing, never truncate), find the
    // end, write there (positional write, which is "seek to end + write" without moving a shared cursor).
    const end = try f.length(io);
    var small: [4096]u8 = undefined;
    if (line.len < small.len) {
        @memcpy(small[0..line.len], line);
        small[line.len] = '\n';
        try f.writePositionalAll(io, small[0 .. line.len + 1], end); // one write
    } else {
        try f.writePositionalAll(io, line, end);
        try f.writePositionalAll(io, "\n", end + line.len);
    }
    try f.sync(io);
}

/// Append one line (a newline is added) to `path`, creating it when missing: under the exclusive lock (so concurrent
/// appenders, in this or another process, cannot split or overwrite each other's line) and `fsync`ed. Refuses a
/// symlink at `path`.
pub fn appendLine(io: Io, dir: Io.Dir, path: []const u8, line: []const u8) !void {
    const lk = try DocLock.acquire(io, dir, path);
    defer lk.release(io);
    try lk.appendLine(io, line);
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
    // SPEC 19: acknowledged warnings (the summary's `INFO I_ACK` lines) are logged with their reasons
    var ack_n: usize = 0;
    var lines = std.mem.splitScalar(u8, summary, '\n');
    while (lines.next()) |ln| {
        const l = std.mem.trimEnd(u8, ln, "\r");
        if (!std.mem.startsWith(u8, l, "INFO I_ACK")) continue;
        try out.appendSlice(a, if (ack_n == 0) ",\"ack\":[" else ",");
        try kerf.json.writeString(&out, a, l);
        ack_n += 1;
    }
    if (ack_n > 0) try out.append(a, ']');
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

test "V-15: Windows device names and trailing dots/spaces are not document names" {
    try std.testing.expect(!validDocFile("CON.kerf.json"));
    try std.testing.expect(!validDocFile("con.kerf.json"));
    try std.testing.expect(!validDocFile("NUL.kerf.json"));
    try std.testing.expect(!validDocFile("aux.kerf.json"));
    try std.testing.expect(!validDocFile("PRN.kerf.json"));
    try std.testing.expect(!validDocFile("COM1.kerf.json"));
    try std.testing.expect(!validDocFile("lpt9.kerf.json"));
    try std.testing.expect(!validDocFile("CON.x.kerf.json")); // Windows ignores everything after the first dot
    try std.testing.expect(!validDocFile("a..kerf.json")); // stem ends with a dot
    try std.testing.expect(!validDocFile("a .kerf.json")); // stem ends with a space
    try std.testing.expect(!validDocFile(" a.kerf.json"));
    try std.testing.expect(validDocFile("COM0.kerf.json"));
    try std.testing.expect(validDocFile("console.kerf.json"));
    try std.testing.expect(validDocFile("COM10.kerf.json"));
    try std.testing.expect(validDocFile("a.b.kerf.json"));
}

test "appendLine refuses a symlink and DocLock appends through its handle" {
    var threaded: Io.Threaded = .init_single_threaded;
    const io = threaded.io();
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    if (builtin.os.tag != .windows) {
        try tmp.dir.writeFile(io, .{ .sub_path = "target.txt", .data = "keep\n" });
        try tmp.dir.symLink(io, "target.txt", "a.kerf.json.log.jsonl", .{});
        try std.testing.expectError(error.SymLink, appendLine(io, tmp.dir, "a.kerf.json.log.jsonl", "{\"n\":1}"));
        var buf: [16]u8 = undefined;
        const got = try tmp.dir.readFile(io, "target.txt", &buf);
        try std.testing.expectEqualStrings("keep\n", got);
    }
    const lk = try DocLock.acquire(io, tmp.dir, "b.kerf.json.log.jsonl");
    try lk.appendLine(io, "{\"n\":1}");
    try lk.appendLine(io, "{\"n\":2}");
    lk.release(io);
    var buf2: [64]u8 = undefined;
    try std.testing.expectEqualStrings("{\"n\":1}\n{\"n\":2}\n", try tmp.dir.readFile(io, "b.kerf.json.log.jsonl", &buf2));
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
