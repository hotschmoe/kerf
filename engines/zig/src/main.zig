const std = @import("std");
const kerf = @import("kerf");
const workspace = @import("workspace.zig");
const serve = @import("serve.zig");

const usage =
    \\kerf: conversational construction details (https://github.com/hotschmoe/kerf)
    \\
    \\usage:
    \\  kerf guide                       instructions for LLM agents + component catalog (start here)
    \\  kerf init [dir]                  make a details folder agent-ready (AGENTS.md + CLAUDE.md)
    \\  kerf schema [topic]              field reference: doc view note dim label cite ops <component type> (no topic: list)
    \\  kerf new <file> [--id ID] [--title "TITLE"] [--template section]
    \\  kerf apply <doc> <ops.json|-> [-w] [--why "REASON"] [-o out]   apply ops (file or stdin); -w writes back to <doc>
    \\  kerf apply <doc> --ops '<json>' [-w] [--why "REASON"] [--dry-run]
    \\      a failed apply prints "ERROR <code> <path>: <message>  Fix: <fix>" per error on stderr, then "nothing written" (exit 1)
    \\      -w also appends one line to <doc>.log.jsonl (who = $KERF_ACTOR or "agent"; --why = the reason, shown to the designer)
    \\  kerf serve [--dir .] [--host 127.0.0.1] [--port 7700] [--open] [--token T | --no-token]
    \\      local workspace server: web UI + /api over the folder of *.kerf.json (see spec/SERVE.md)
    \\  kerf check <doc>                 summary + diagnostics (exit 1 on errors)
    \\  kerf export <doc> --view A --format png|svg|dxf|pdf [--px 1600] [--sheet] -o <file>
    \\  kerf catalog [--markdown]
    \\  kerf fmt <doc> [-w]
    \\  kerf drawing <doc> --view A [-o out.json]
    \\  kerf mesh <doc> [-o mesh.json]
    \\  kerf call <fn> < input.json      raw engine API (JSON in, JSON/bytes out); `kerf call help` lists functions + input shapes
    \\  kerf version
    \\common: [--style file.kerfstyle.json]
    \\
;
const cli_guide = @embedFile("kerf_cli_guide");
const system_md = @embedFile("kerf_system_md");

/// Fold text to pure ASCII for Windows consoles (PowerShell mangles UTF-8): dashes, quotes, x-sign, degree sign,
/// vulgar fractions and NBSP get readable ASCII; anything else becomes `?`.
pub fn asciiFold(a: std.mem.Allocator, text: []const u8) ![]u8 {
    var out: std.ArrayList(u8) = try .initCapacity(a, text.len);
    errdefer out.deinit(a);
    const view = std.unicode.Utf8View.init(text) catch return error.InvalidUtf8;
    var it = view.iterator();
    while (it.nextCodepoint()) |cp| {
        if (cp < 0x80) {
            try out.append(a, @intCast(cp));
            continue;
        }
        const rep: []const u8 = switch (cp) {
            0x2010...0x2015, 0x2212, 0x2043 => "-",
            0x2018, 0x2019, 0x201A, 0x2032 => "'",
            0x201C, 0x201D, 0x201E, 0x2033 => "\"",
            0xD7 => "x",
            0xB0 => " deg",
            0xBD => " 1/2",
            0xBC => " 1/4",
            0xBE => " 3/4",
            0x215B => " 1/8",
            0x215C => " 3/8",
            0x215D => " 5/8",
            0x215E => " 7/8",
            0xA0, 0x2009, 0x202F, 0x2002, 0x2003 => " ",
            0x2026 => "...",
            0x2022, 0xB7 => "*",
            0x2192 => "->",
            0x2190 => "<-",
            0x2264 => "<=",
            0x2265 => ">=",
            0x2248 => "~",
            0xB1 => "+/-",
            else => "?",
        };
        try out.appendSlice(a, rep);
    }
    return out.toOwnedSlice(a);
}

test "asciiFold" {
    const a = std.testing.allocator;
    const got = try asciiFold(a, "2\u{d7}4 \u{2014} 3\u{bd}\" \u{201c}x\u{201d} 45\u{b0} \u{2026}\u{4e2d}");
    defer a.free(got);
    try std.testing.expectEqualStrings("2x4 - 3 1/2\" \"x\" 45 deg ...?", got);
    for (got) |c| try std.testing.expect(c < 0x80);
}

test "embedded guide sources are already pure ASCII (the fold is a safety net)" {
    for (cli_guide ++ system_md) |c| try std.testing.expect(c < 0x80);
}

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
    const ctx = Ctx{ .actor = init.environ_map.get("KERF_ACTOR") orelse "agent", .environ_map = init.environ_map };
    const code = run(gpa, io, args.items, &stderr.interface, ctx) catch |e| blk: {
        stderr.interface.print("kerf: {s}\n", .{@errorName(e)}) catch {};
        break :blk @as(u8, 2);
    };
    stderr.interface.flush() catch {};
    if (code != 0) std.process.exit(code);
}

/// Process context: who is acting (op log `who`) and the environment (agent bridge PATH/children).
pub const Ctx = struct {
    actor: []const u8,
    environ_map: *const std.process.Environ.Map,
};

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
    ops: ?[]const u8 = null,
    id: ?[]const u8 = null,
    title: ?[]const u8 = null,
    px: ?[]const u8 = null,
    why: ?[]const u8 = null,
    dry_run: bool = false,
    template: ?[]const u8 = null,
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
        if (std.mem.eql(u8, a, "--view")) o.view = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--format")) o.format = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--style")) o.style = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "-o")) o.out = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--sheet")) o.sheet = true else if (std.mem.eql(u8, a, "--markdown")) o.markdown = true else if (std.mem.eql(u8, a, "-w")) o.write = true else if (std.mem.eql(u8, a, "--ops")) o.ops = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--id")) o.id = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--title")) o.title = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--px")) o.px = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--why")) o.why = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "--dry-run")) o.dry_run = true else if (std.mem.eql(u8, a, "--template")) o.template = try next(args, &i, err, a) else if (std.mem.eql(u8, a, "-h") or std.mem.eql(u8, a, "--help")) return error.Usage else if (a.len > 1 and a[0] == '-') {
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

fn appendJsonString(a: std.mem.Allocator, out: *std.ArrayList(u8), str: []const u8) !void {
    try out.append(a, '"');
    for (str) |c| switch (c) {
        '"' => try out.appendSlice(a, "\\\""),
        '\\' => try out.appendSlice(a, "\\\\"),
        '\n' => try out.appendSlice(a, "\\n"),
        0...9, 11...31 => try out.print(a, "\\u{x:0>4}", .{c}),
        else => try out.append(a, c),
    };
    try out.append(a, '"');
}

/// Appends the op-log line. The document is already written (atomically), so a failure here is reported on stderr
/// (what failed, that the document is fine) and the caller exits 3. Returns true when the log line was appended.
fn logWrite(a: std.mem.Allocator, io: std.Io, doc_path: []const u8, meta: workspace.LogMeta, ops_text: []const u8, changed: []const []const u8, summary: []const u8) bool {
    const lp = workspace.logPath(a, doc_path) catch return logFail(io, doc_path, "out of memory");
    const line = workspace.buildEntry(a, io, meta, ops_text, changed, summary) catch return logFail(io, lp, "could not build the log entry");
    workspace.appendLine(io, std.Io.Dir.cwd(), lp, line) catch |e| return logFail(io, lp, @errorName(e));
    return true;
}

fn logFail(io: std.Io, lp: []const u8, what: []const u8) bool {
    var b: [512]u8 = undefined;
    var w = std.Io.File.stderr().writer(io, &b);
    w.interface.print("kerf: error: could not append to op log {s}: {s}. The document itself WAS written; only the log entry is missing. Exit code 3.\n", .{ lp, what }) catch {};
    w.interface.flush() catch {};
    return false;
}

fn createOp(a: std.mem.Allocator, file: []const u8, id: []const u8, title: []const u8) ![]u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "[{\"op\":\"create\",\"file\":");
    try appendJsonString(a, &out, file);
    try out.appendSlice(a, ",\"id\":");
    try appendJsonString(a, &out, id);
    try out.appendSlice(a, ",\"title\":");
    try appendJsonString(a, &out, title);
    try out.appendSlice(a, "}]");
    return out.items;
}

/// `ERROR <code> <path>: <message>  Fix: <fix>` (SPEC 19). `path` falls back to the diagnostic id, then `-`.
fn printDiagError(err: *std.Io.Writer, d: kerf.json.Value) !void {
    const code = if (d.get("code")) |c| (c.str() orelse "E_UNKNOWN") else "E_UNKNOWN";
    const path_s: []const u8 = blk: {
        if (d.get("path")) |p| if (p.str()) |ps| if (ps.len > 0) break :blk ps;
        if (d.get("id")) |p| if (p.str()) |ps| if (ps.len > 0) break :blk ps;
        break :blk "-";
    };
    const msg = if (d.get("message")) |m| (m.str() orelse "") else "";
    try err.print("ERROR {s} {s}: {s}", .{ code, path_s, msg });
    if (d.get("fix")) |f| if (f.str()) |fs| if (fs.len > 0) try err.print("  Fix: {s}", .{fs});
    try err.writeAll("\n");
}

/// Print an engine failure `{"error": {code, message}}` as `ERROR <code>: <message>`; falls back to the raw bytes.
fn printApiError(a: std.mem.Allocator, err: *std.Io.Writer, bytes: []const u8) !void {
    var perr: kerf.json.ParseError = undefined;
    if (try kerf.json.parse(a, bytes, &perr)) |v| if (v.get("error")) |e| {
        const code = if (e.get("code")) |c| (c.str() orelse "E_UNKNOWN") else "E_UNKNOWN";
        const msg = if (e.get("message")) |m| (m.str() orelse "") else "";
        try err.print("ERROR {s}: {s}\n", .{ code, msg });
        return;
    };
    try err.writeAll(bytes);
}

fn run(gpa: std.mem.Allocator, io: std.Io, args: []const []const u8, err: *std.Io.Writer, ctx: Ctx) !u8 {
    if (args.len == 0) {
        try err.writeAll(usage);
        return 2;
    }
    const cmd = args[0];
    if (std.mem.eql(u8, cmd, "serve")) return serve.cliMain(gpa, io, args[1..], err, ctx.environ_map);
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
        if (o.markdown and r.ok) {
            const folded = try asciiFold(gpa, r.bytes);
            defer gpa.free(folded);
            try writeOut(io, o.out, folded);
            return 0;
        }
        try writeOut(io, o.out, r.bytes);
        return if (r.ok) 0 else 1;
    }
    if (std.mem.eql(u8, cmd, "schema")) {
        var sarena = std.heap.ArenaAllocator.init(gpa);
        defer sarena.deinit();
        const sa = sarena.allocator();
        var inp: std.ArrayList(u8) = .empty;
        try inp.appendSlice(sa, "{");
        if (o.npos >= 1) {
            try inp.appendSlice(sa, "\"topic\":");
            try appendJsonString(sa, &inp, o.pos[0]);
        }
        try inp.appendSlice(sa, "}");
        const r = try kerf.call(gpa, "schema", inp.items);
        defer gpa.free(r.bytes);
        if (!r.ok) {
            try printApiError(sa, err, r.bytes);
            return 1;
        }
        var sperr: kerf.json.ParseError = undefined;
        const res = (try kerf.json.parse(sa, r.bytes, &sperr)) orelse return error.BadEngineOutput;
        const text = (if (res.get("text")) |t| t.str() else null) orelse return error.BadEngineOutput;
        const folded = try asciiFold(gpa, text);
        defer gpa.free(folded);
        try writeOut(io, o.out, folded);
        return 0;
    }
    if (std.mem.eql(u8, cmd, "help") or std.mem.eql(u8, cmd, "--help") or std.mem.eql(u8, cmd, "-h")) {
        try writeOut(io, null, usage);
        return 0;
    }
    if (std.mem.eql(u8, cmd, "guide")) {
        const r = try kerf.call(gpa, "catalog", "{\"format\":\"markdown\"}");
        defer gpa.free(r.bytes);
        var text: std.ArrayList(u8) = .empty;
        defer text.deinit(gpa);
        var garena = std.heap.ArenaAllocator.init(gpa);
        defer garena.deinit();
        try text.appendSlice(gpa, cli_guide);
        try text.appendSlice(gpa, "\n");
        try text.appendSlice(gpa, try kerf.api.schema.guideSection(garena.allocator()));
        try text.appendSlice(gpa, "\n# Drafting instructions\n\n");
        try text.appendSlice(gpa, system_md);
        try text.appendSlice(gpa, "\n# Component catalog\n\n");
        try text.appendSlice(gpa, r.bytes);
        const folded = try asciiFold(gpa, text.items);
        defer gpa.free(folded);
        try writeOut(io, o.out, folded);
        return 0;
    }
    if (std.mem.eql(u8, cmd, "init")) {
        const dir_path = if (o.npos >= 1) o.pos[0] else ".";
        std.Io.Dir.cwd().createDirPath(io, dir_path) catch {};
        var d = try std.Io.Dir.cwd().openDir(io, dir_path, .{});
        defer d.close(io);
        const agents_md =
            \\# Kerf details library
            \\
            \\This folder holds construction details as `*.kerf.json` files, built with the `kerf` CLI.
            \\
            \\Before creating or editing any detail, run `kerf guide` and follow it exactly. It holds the
            \\workflow, drafting rules, note grammar, and component catalog. Edit details only via
            \\`kerf apply <file> ... -w`, never by hand. After changes, export a PNG
            \\(`kerf export <file> --view A --format png -o <file>-A.png`) and look at it before reporting.
            \\The designer reviews and exports in the Kerf web UI.
            \\
        ;
        var wrote: usize = 0;
        for ([_][]const u8{ "AGENTS.md", "CLAUDE.md" }) |name| {
            if (d.access(io, name, .{})) |_| {
                try writeOut(io, null, "kept existing ");
                try writeOut(io, null, name);
                try writeOut(io, null, "\n");
            } else |_| {
                try d.writeFile(io, .{ .sub_path = name, .data = if (std.mem.eql(u8, name, "CLAUDE.md")) "@AGENTS.md\n" else agents_md });
                wrote += 1;
                try writeOut(io, null, "wrote ");
                try writeOut(io, null, name);
                try writeOut(io, null, "\n");
            }
        }
        try writeOut(io, null, "ready: open Claude Code / Grok in this folder and ask for a detail.\n");
        return 0;
    }
    if (std.mem.eql(u8, cmd, "new")) {
        if (o.npos < 1) {
            try err.writeAll("kerf new: needs <file>, e.g. kerf new truss-cmu.kerf.json --title \"TRUSS BEARING\"\n");
            return 2;
        }
        const path = o.pos[0];
        if (std.Io.Dir.cwd().access(io, path, .{})) |_| {
            try err.print("kerf new: {s} already exists (refusing to overwrite)\n", .{path});
            return 1;
        } else |_| {}
        var arena = std.heap.ArenaAllocator.init(gpa);
        defer arena.deinit();
        const a = arena.allocator();
        const stem = workspace.docStem(path);
        const text = if (o.template) |tpl| blk: {
            if (!std.mem.eql(u8, tpl, "section")) {
                try err.print("kerf new: unknown template \"{s}\"; available: section (a section view with two members, two notes, a dim, a label)\n", .{tpl});
                return 2;
            }
            var id: std.ArrayList(u8) = .empty;
            for (o.id orelse stem) |c| try id.append(a, if (std.ascii.isAlphanumeric(c)) std.ascii.toLower(c) else '-');
            break :blk try kerf.api.schema.templateText(a, id.items, o.title orelse "");
        } else try workspace.newDocText(a, o.id orelse stem, o.title orelse "");
        try workspace.writeFileAtomic(io, std.Io.Dir.cwd(), path, text);
        const logged = logWrite(a, io, path, .{ .who = ctx.actor, .tool = "kerf-cli", .why = "create" }, try createOp(a, std.fs.path.basename(path), o.id orelse stem, o.title orelse ""), &.{}, workspace.checkSummary(a, text) catch "");
        try writeOut(io, null, "created ");
        try writeOut(io, null, path);
        try writeOut(io, null, if (o.template != null) "\nnext: edit it with kerf apply <file> ops.json -w   (see `kerf schema`, `kerf guide`)\n" else "\nnext: kerf apply <file> ops.json -w   (see `kerf guide`)\n");
        return if (logged) 0 else 3;
    }
    if (std.mem.eql(u8, cmd, "call")) {
        if (o.npos < 1) {
            try err.writeAll(usage);
            return 2;
        }
        var stdin_buf: [8192]u8 = undefined;
        var rd = std.Io.File.stdin().reader(io, &stdin_buf);
        // `kerf call help` needs no input: do not block on an interactive stdin
        const input = if (std.mem.eql(u8, o.pos[0], "help")) try gpa.dupe(u8, "{}") else try rd.interface.allocRemaining(gpa, .limited(256 << 20));
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
    var ops_text_for_log: []const u8 = "[]";
    var ops_keep: ?[]u8 = null; // the ops text outlives the branch that read it (the op log is written later)
    defer if (ops_keep) |b| gpa.free(b);
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
        if (o.px) |px| try extra.print(gpa, ",\"px\":{s}", .{px});
        if (o.out == null) {
            try err.writeAll("kerf export: -o <file> is required\n");
            return 2;
        }
    } else if (std.mem.eql(u8, cmd, "apply")) {
        var ops_owned: ?[]u8 = null;
        defer if (ops_owned) |b| gpa.free(b);
        const ops: []const u8 = if (o.ops) |inline_ops| inline_ops else if (o.npos >= 2 and !std.mem.eql(u8, o.pos[1], "-")) blk: {
            ops_owned = try readFile(gpa, io, o.pos[1]);
            break :blk ops_owned.?;
        } else if (o.npos >= 2) blk: {
            var stdin_buf: [8192]u8 = undefined;
            var rd = std.Io.File.stdin().reader(io, &stdin_buf);
            ops_owned = try rd.interface.allocRemaining(gpa, .limited(256 << 20));
            break :blk ops_owned.?;
        } else {
            try err.writeAll("kerf apply: needs ops: <doc> <ops.json>, <doc> - (stdin), or --ops '<json>'\n");
            return 2;
        };
        if (o.dry_run and o.write) {
            try err.writeAll("kerf apply: --dry-run and -w contradict each other (dry-run writes nothing)\n");
            return 2;
        }
        {
            var vperr: kerf.json.ParseError = undefined;
            var varena = std.heap.ArenaAllocator.init(gpa);
            defer varena.deinit();
            const parsed = try kerf.json.parse(varena.allocator(), std.mem.trim(u8, ops, " \t\r\n"), &vperr);
            if (parsed == null) {
                try err.print("ERROR E_JSON ops: the ops are not valid JSON: {s} (line {d}, column {d})  Fix: pass a JSON array of ops, e.g. [{{\"op\":\"update\",\"path\":\"components/x\",\"value\":{{...}}}}]; on PowerShell write the ops to a file\nnothing written\n", .{ vperr.msg, vperr.line, vperr.col });
                return 1;
            }
        }
        ops_keep = try gpa.dupe(u8, ops);
        ops_text_for_log = ops_keep.?;
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
        var earena = std.heap.ArenaAllocator.init(gpa);
        defer earena.deinit();
        try printApiError(earena.allocator(), err, r.bytes);
        if (std.mem.eql(u8, cmd, "apply")) try err.writeAll("nothing written\n");
        return 1;
    }
    if (std.mem.eql(u8, cmd, "fmt")) {
        // r.bytes is {"doc": ..., "text": "<canonical text>"}: write the canonical text itself
        var farena = std.heap.ArenaAllocator.init(gpa);
        defer farena.deinit();
        var fperr: kerf.json.ParseError = undefined;
        const fres = (try kerf.json.parse(farena.allocator(), r.bytes, &fperr)) orelse return error.BadEngineOutput;
        const canon_text = (if (fres.get("text")) |t| t.str() else null) orelse return error.BadEngineOutput;
        var text: std.ArrayList(u8) = .empty;
        defer text.deinit(gpa);
        try text.appendSlice(gpa, canon_text);
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
        if (res.get("summary")) |sv| if (sv.str()) |txt| try writeOut(io, null, txt);
        if (std.mem.eql(u8, cmd, "check")) {
            // diagnostics already appear in the summary; exit 1 on errors
            const dl = (res.get("diagnostics") orelse kerf.json.Value{ .array = &.{} }).arr() orelse &.{};
            for (dl) |d| if (d.get("level")) |lv| if (lv.str()) |ls| if (std.mem.eql(u8, ls, "error")) return 1;
            return 0;
        }
        const ok = if (res.get("ok")) |v| (v == .bool and v.bool) else false;
        if (!ok) {
            // SPEC 19: every error diagnostic on stderr, then `nothing written`
            const dl = (res.get("diagnostics") orelse kerf.json.Value{ .array = &.{} }).arr() orelse &.{};
            for (dl) |d| if (d.get("level")) |lv| if (lv.str()) |ls| if (std.mem.eql(u8, ls, "error")) try printDiagError(err, d);
            try err.writeAll("nothing written\n");
            return 1;
        }
        var logged = true;
        if (res.get("doc")) |dv| {
            const text = try kerf.canon.write(a, dv);
            if (ok) {
                if (o.write and !o.dry_run) {
                    try workspace.writeFileAtomic(io, std.Io.Dir.cwd(), doc_path, text);
                    var changed: std.ArrayList([]const u8) = .empty;
                    if (res.get("changed")) |cv| if (cv.arr()) |items| for (items) |it| if (it.str()) |cs| try changed.append(a, cs);
                    const sum = if (res.get("summary")) |sv| (sv.str() orelse "") else "";
                    logged = logWrite(a, io, doc_path, .{ .who = ctx.actor, .tool = "kerf-cli", .why = o.why orelse "" }, std.mem.trim(u8, ops_text_for_log, " \t\r\n"), changed.items, sum);
                    try writeOut(io, null, "wrote ");
                    try writeOut(io, null, doc_path);
                    try writeOut(io, null, "\n");
                } else if (o.dry_run) {
                    try writeOut(io, null, "(dry run: nothing written)\n");
                } else if (o.out) |_| {
                    try writeOut(io, o.out, text);
                } else {
                    try writeOut(io, null, "(dry run: add -w to write the result back to the document)\n");
                }
            }
        }
        if (ok and !logged) return 3;
        return if (ok) 0 else 1;
    }
    try writeOut(io, o.out, r.bytes);
    return 0;
}
