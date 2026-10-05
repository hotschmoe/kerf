const std = @import("std");
const kerf = @import("kerf");

const usage =
    \\usage:
    \\  kerf version
    \\  kerf catalog [--markdown]
    \\  kerf fmt <doc> [-w]
    \\  kerf check <doc> [--style S]
    \\  kerf apply <doc> <ops.json> [--style S] [-o out.kerf.json]
    \\  kerf drawing <doc> --view A [--style S] [-o out.json]
    \\  kerf export <doc> --view A --format svg|dxf|pdf [--style S] [--sheet] -o <file>
    \\  kerf mesh <doc> [-o mesh.json]
    \\  kerf call <fn> < input.json
    \\
;

pub fn main(init: std.process.Init) !void {
    const gpa = init.gpa;
    const io = init.io;
    var args_it = try init.minimal.args.iterateAllocator(gpa);
    defer args_it.deinit();
    var args: std.ArrayList([]const u8) = .empty;
    defer args.deinit(gpa);
    _ = args_it.next();
    while (args_it.next()) |a| try args.append(gpa, a);
    var buf: [4096]u8 = undefined;
    var stderr = std.Io.File.stderr().writer(io, &buf);
    const code = run(gpa, io, args.items, &stderr.interface) catch |e| blk: {
        stderr.interface.print("kerf: {s}\n", .{@errorName(e)}) catch {};
        break :blk @as(u8, 2);
    };
    stderr.interface.flush() catch {};
    if (code != 0) std.process.exit(code);
}

fn readFile(gpa: std.mem.Allocator, io: std.Io, path: []const u8) ![]u8 {
    return std.Io.Dir.cwd().readFileAlloc(io, path, gpa, .limited(256 << 20));
}

fn writeOut(io: std.Io, path: ?[]const u8, bytes: []const u8) !void {
    if (path) |p| {
        try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = p, .data = bytes });
    } else {
        var b: [8192]u8 = undefined;
        var w = std.Io.File.stdout().writer(io, &b);
        try w.interface.writeAll(bytes);
        try w.interface.flush();
    }
}

const Opts = struct {
    view: ?[]const u8 = null,
    format: ?[]const u8 = null,
    style: ?[]const u8 = null,
    out: ?[]const u8 = null,
    sheet: bool = false,
    markdown: bool = false,
    write: bool = false,
    pos: [4][]const u8 = undefined,
    npos: usize = 0,
};

fn parseOpts(args: []const []const u8, err: *std.Io.Writer) !Opts {
    var o = Opts{};
    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        const a = args[i];
        const next = struct {
            fn get(all: []const []const u8, idx: *usize, e: *std.Io.Writer, flag: []const u8) ![]const u8 {
                if (idx.* + 1 >= all.len) {
                    try e.print("kerf: {s} needs a value\n", .{flag});
                    return error.Usage;
                }
                idx.* += 1;
                return all[idx.*];
            }
        }.get;
        if (std.mem.eql(u8, a, "--view")) o.view = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--format")) o.format = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--style")) o.style = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "-o")) o.out = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--sheet")) o.sheet = true else if (std.mem.eql(u8, a, "--markdown")) o.markdown = true else if (std.mem.eql(u8, a, "-w")) o.write = true else if (a.len > 0 and a[0] == '-') {
            try err.print("kerf: unknown option {s}\n", .{a});
            return error.Usage;
        } else {
            if (o.npos >= o.pos.len) return error.Usage;
            o.pos[o.npos] = a;
            o.npos += 1;
        }
    }
    return o;
}

/// Build `{"doc": <file>, "style": <file>?, ...extra}` from raw document text.
fn buildInput(gpa: std.mem.Allocator, io: std.Io, doc_path: []const u8, style_path: ?[]const u8, extra: []const u8) ![]u8 {
    const doc = try readFile(gpa, io, doc_path);
    defer gpa.free(doc);
    var out: std.ArrayList(u8) = .empty;
    errdefer out.deinit(gpa);
    try out.appendSlice(gpa, "{\"doc\":");
    try out.appendSlice(gpa, std.mem.trim(u8, doc, " \t\r\n"));
    if (style_path) |sp| {
        const st = try readFile(gpa, io, sp);
        defer gpa.free(st);
        try out.appendSlice(gpa, ",\"style\":");
        try out.appendSlice(gpa, std.mem.trim(u8, st, " \t\r\n"));
    }
    if (extra.len > 0) {
        try out.append(gpa, ',');
        try out.appendSlice(gpa, extra);
    }
    try out.append(gpa, '}');
    return out.toOwnedSlice(gpa);
}

fn run(gpa: std.mem.Allocator, io: std.Io, args: []const []const u8, err: *std.Io.Writer) !u8 {
    if (args.len == 0) {
        try err.writeAll(usage);
        return 2;
    }
    const cmd = args[0];
    const o = parseOpts(args[1..], err) catch {
        try err.writeAll(usage);
        return 2;
    };
    if (std.mem.eql(u8, cmd, "version")) {
        const r = try kerf.call(gpa, "version", "{}");
        defer gpa.free(r.bytes);
        try writeOut(io, null, r.bytes);
        return 0;
    }
    if (std.mem.eql(u8, cmd, "catalog")) {
        const r = try kerf.call(gpa, "catalog", if (o.markdown) "{\"format\":\"markdown\"}" else "{\"format\":\"json\"}");
        defer gpa.free(r.bytes);
        try writeOut(io, o.out, r.bytes);
        return if (r.ok) 0 else 1;
    }
    if (std.mem.eql(u8, cmd, "call")) {
        if (o.npos < 1) {
            try err.writeAll(usage);
            return 2;
        }
        var stdin_buf: [8192]u8 = undefined;
        var rd = std.Io.File.stdin().reader(io, &stdin_buf);
        const input = try rd.interface.allocRemaining(gpa, .limited(256 << 20));
        defer gpa.free(input);
        const r = try kerf.call(gpa, o.pos[0], input);
        defer gpa.free(r.bytes);
        try writeOut(io, o.out, r.bytes);
        return if (r.ok) 0 else 1;
    }
    if (o.npos < 1) {
        try err.print("kerf {s}: missing <doc>\n", .{cmd});
        try err.writeAll(usage);
        return 2;
    }
    const doc_path = o.pos[0];
    var extra: std.ArrayList(u8) = .empty;
    defer extra.deinit(gpa);
    var fname: []const u8 = cmd;
    if (std.mem.eql(u8, cmd, "drawing")) {
        const v = o.view orelse {
            try err.writeAll("kerf drawing: --view <id> is required\n");
            return 2;
        };
        try extra.print(gpa, "\"view\":\"{s}\"", .{v});
    } else if (std.mem.eql(u8, cmd, "export")) {
        const v = o.view orelse {
            try err.writeAll("kerf export: --view <id> is required\n");
            return 2;
        };
        const f = o.format orelse "svg";
        try extra.print(gpa, "\"view\":\"{s}\",\"format\":\"{s}\",\"sheet\":{s}", .{ v, f, if (o.sheet) "true" else "false" });
        if (o.out == null) {
            try err.writeAll("kerf export: -o <file> is required\n");
            return 2;
        }
    } else if (std.mem.eql(u8, cmd, "apply")) {
        if (o.npos < 2) {
            try err.writeAll("kerf apply: needs <doc> <ops.json>\n");
            return 2;
        }
        const ops = try readFile(gpa, io, o.pos[1]);
        defer gpa.free(ops);
        try extra.print(gpa, "\"ops\":{s}", .{std.mem.trim(u8, ops, " \t\r\n")});
    } else if (!(std.mem.eql(u8, cmd, "fmt") or std.mem.eql(u8, cmd, "check") or std.mem.eql(u8, cmd, "mesh"))) {
        try err.print("kerf: unknown command '{s}'\n", .{cmd});
        try err.writeAll(usage);
        return 2;
    }
    fname = cmd;
    const input = try buildInput(gpa, io, doc_path, o.style, extra.items);
    defer gpa.free(input);
    const r = try kerf.call(gpa, fname, input);
    defer gpa.free(r.bytes);
    if (!r.ok) {
        try err.writeAll(r.bytes);
        return 1;
    }
    if (std.mem.eql(u8, cmd, "fmt")) {
        // r.bytes is {"doc": ...}; strip the wrapper for file output
        const body = std.mem.trim(u8, r.bytes, " \t\r\n");
        const inner = body["{\"doc\":".len .. body.len - 1];
        var text: std.ArrayList(u8) = .empty;
        defer text.deinit(gpa);
        try text.appendSlice(gpa, inner);
        try text.append(gpa, '\n');
        if (o.write) {
            try writeOut(io, doc_path, text.items);
        } else try writeOut(io, o.out, text.items);
        return 0;
    }
    if (std.mem.eql(u8, cmd, "apply") or std.mem.eql(u8, cmd, "check")) {
        var arena = std.heap.ArenaAllocator.init(gpa);
        defer arena.deinit();
        const a = arena.allocator();
        var perr: kerf.json.ParseError = undefined;
        const res = (try kerf.json.parse(a, r.bytes, &perr)) orelse return error.BadEngineOutput;
        if (res.get("summary")) |sv| if (sv.str()) |txt| try err.writeAll(txt);
        if (std.mem.eql(u8, cmd, "check")) {
            // diagnostics already appear in the summary; exit 1 on errors
            const dl = (res.get("diagnostics") orelse kerf.json.Value{ .array = &.{} }).arr() orelse &.{};
            for (dl) |d| if (d.get("level")) |lv| if (lv.str()) |ls| if (std.mem.eql(u8, ls, "error")) return 1;
            return 0;
        }
        const ok = if (res.get("ok")) |v| (v == .bool and v.bool) else false;
        if (res.get("doc")) |dv| {
            const text = try kerf.canon.write(a, dv);
            if (ok) {
                try writeOut(io, o.out, text);
            }
        }
        return if (ok) 0 else 1;
    }
    try writeOut(io, o.out, r.bytes);
    return 0;
}
