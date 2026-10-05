//! Chat glue: the TEA-side half of the Claude harness. Interprets the
//! harness `Step`s (declare an HTTP effect, run tools synchronously against
//! the document session, schedule a retry) and feeds results back in.

const std = @import("std");
const teak = @import("teak");
const alloc = @import("alloc.zig");
const model = @import("model.zig");
const flow = @import("docflow.zig");
const fx = @import("fx.zig");
const llm = @import("../llm/mod.zig");
const draw = @import("../draw/mod.zig");

const Model = model.Model;
const gpa = alloc.gpa;
const tools = llm.tools;

pub fn hasKey(m: *const Model) bool {
    return m.key_len > 0;
}

pub fn modelId(p: model.Model3) []const u8 {
    return switch (p) {
        .opus_5_5 => llm.types.Model.opus_5_5.id(),
        .sonnet_5_5 => llm.types.Model.sonnet_5_5.id(),
    };
}

pub fn modelLabel(p: model.Model3) []const u8 {
    return switch (p) {
        .opus_5_5 => llm.types.Model.opus_5_5.label(),
        .sonnet_5_5 => llm.types.Model.sonnet_5_5.label(),
    };
}

pub fn applyConfig(m: *Model) void {
    m.chat.session.cfg.model = modelId(m.model_pick);
    if (m.key_len > 0) m.chat.setApiKey(m.key_buf[0..m.key_len]) else m.chat.setApiKey("");
}

pub fn setKey(m: *Model, key: []const u8) void {
    const k = std.mem.trim(u8, key, " \r\n\t");
    const n = @min(k.len, m.key_buf.len);
    @memcpy(m.key_buf[0..n], k[0..n]);
    m.key_len = @intCast(n);
    applyConfig(m);
}

fn engineCtx(m: *Model) tools.Engine {
    return .{ .ctx = m, .apply_fn = toolApply, .inspect_fn = toolInspect, .render_fn = toolRender };
}

// ── sending ────────────────────────────────────────────────────────

pub fn submit(m: *Model) void {
    const text = std.mem.trim(u8, m.chat_ed.content(), " ");
    if (text.len == 0 and m.n_attach == 0) return;
    if (m.chat.session.isBusy()) {
        m.setStatus("CLAUDE IS BUSY", .{});
        return;
    }
    // Images.
    var imgs: [model.MAX_ATTACH]llm.types.Image = undefined;
    for (m.attachments[0..m.n_attach], 0..) |a, i| {
        imgs[i] = .{
            .media_type = if (a.mime_jpeg) .jpeg else .png,
            .data = a.bytes,
            .width = a.width,
            .height = a.height,
            .orig_width = a.orig_width,
            .orig_height = a.orig_height,
        };
    }
    // Designer edits since the last turn.
    var note_buf: ?[]u8 = null;
    defer if (note_buf) |b| gpa.free(b);
    if (m.edit_notes.items.len > 0) {
        note_buf = llm.request.formatEditsNote(gpa, m.edit_notes.items) catch null;
    }
    const step = m.chat.userSubmit(m.nowMs(), text, imgs[0..m.n_attach], note_buf) catch {
        m.setStatus("OUT OF MEMORY", .{});
        return;
    };
    switch (step) {
        .err, .refusal, .ignored => {}, // a rejected submit keeps the text and the edit notes
        else => {
            m.chat_ed.clear();
            clearAttachments(m);
            for (m.edit_notes.items) |n| gpa.free(n);
            m.edit_notes.clearRetainingCapacity();
        },
    }
    m.stick_bottom = true;
    handleStep(m, step);
}

pub fn clearAttachments(m: *Model) void {
    for (m.attachments[0..m.n_attach]) |a| gpa.free(a.bytes);
    m.n_attach = 0;
}

pub fn cancel(m: *Model) void {
    const step = m.chat.cancel(m.nowMs()) catch return;
    m.demo_pending = null;
    m.retry_in_ticks = -1;
    handleStep(m, step);
}

/// Drive the harness from a Step.
pub fn handleStep(m: *Model, first: llm.Step) void {
    var step = first;
    var guard: u32 = 0;
    while (guard < 64) : (guard += 1) {
        switch (step) {
            .send => |spec| {
                sendHttp(m, spec);
                return;
            },
            .run_tools => |uses| {
                var arena = std.heap.ArenaAllocator.init(gpa);
                defer arena.deinit();
                const a = arena.allocator();
                const results = tools.executeAll(a, engineCtx(m), uses) catch {
                    m.setStatus("TOOL EXECUTION FAILED (OUT OF MEMORY)", .{});
                    return;
                };
                step = m.chat.onToolResults(m.nowMs(), results) catch return;
                continue;
            },
            .retry_after_ms => |r| {
                m.retry_in_ticks = @intCast(@divFloor(r.ms + @as(u32, @intCast(model.TICK_MS)) - 1, @as(u32, @intCast(model.TICK_MS))));
                return;
            },
            .done, .ignored => return,
            .err => |e| {
                m.setStatus("CLAUDE: {s}", .{e.message[0..@min(e.message.len, 80)]});
                return;
            },
            .refusal => return,
        }
    }
}

fn sendHttp(m: *Model, spec: llm.types.HttpRequestSpec) void {
    if (m.demo) {
        // Demo mode: answer locally, one step per tick so the UI shows progress.
        m.demo_pending = llm.Demo.next(spec.body_json);
        // The demo result's body is static data, so no copy is needed.
        return;
    }
    var hs: [8]teak.Header = undefined;
    const n = @min(spec.headers.len, hs.len);
    for (spec.headers[0..n], 0..) |h, i| hs[i] = .{ .name = h.name, .value = h.value };
    if (m.fx.http(.chat_http, .post, spec.url, hs[0..n], spec.body_json, 300_000) == null) {
        m.setStatus("EFFECT QUEUE FULL", .{});
    }
}

/// An HTTP result for the chat arrived (real or demo).
pub fn onHttp(m: *Model, res: llm.types.HttpResult) void {
    const step = m.chat.onHttp(m.nowMs(), res) catch {
        m.setStatus("OUT OF MEMORY", .{});
        return;
    };
    m.stick_bottom = true;
    handleStep(m, step);
}

/// Per-tick: demo responses, retry timers.
pub fn tick(m: *Model) void {
    if (m.demo_pending) |r| {
        m.demo_pending = null;
        onHttp(m, r);
        return;
    }
    if (m.retry_in_ticks > 0) {
        m.retry_in_ticks -= 1;
        if (m.retry_in_ticks == 0) {
            m.retry_in_ticks = -1;
            const step = m.chat.retry(m.nowMs()) catch return;
            handleStep(m, step);
        }
    }
}

// ── tool callbacks ─────────────────────────────────────────────────

fn toolApply(ctx: *anyopaque, a: std.mem.Allocator, ops_json: []const u8, why: []const u8) tools.ApplyOutcome {
    const m: *Model = @ptrCast(@alignCast(ctx));
    const r = m.doc.apply(ops_json, why, .claude, flow.docNowSec(m)) catch return .{ .ok = false, .text = "out of memory" };
    defer gpa.free(r.text);
    const text = a.dupe(u8, r.text) catch "out of memory";
    if (r.ok) {
        flow.afterDocChange(m, m.nViews() == 0 or r.op_count == 0 or std.mem.indexOf(u8, ops_json, "\"path\":\"doc\"") != null);
        flow.setSource(m, "CLAUDE");
        m.setStatus("CLAUDE: {s}", .{why[0..@min(why.len, 80)]});
        m.ready = true;
    }
    return .{ .ok = r.ok, .text = text, .n_err = r.errors, .n_warn = r.warnings };
}

fn toolInspect(ctx: *anyopaque, a: std.mem.Allocator, args_json: []const u8) tools.TextOutcome {
    const m: *Model = @ptrCast(@alignCast(ctx));
    if (!m.doc.hasDoc()) return .{ .ok = false, .text = "no document is loaded yet. Send kerf_apply with a set doc op first." };
    switch (m.doc.inspectJson(args_json)) {
        .ok => |t| {
            defer gpa.free(t);
            return .{ .ok = true, .text = a.dupe(u8, t) catch "out of memory" };
        },
        .err => |t| {
            defer gpa.free(t);
            return .{ .ok = false, .text = a.dupe(u8, t) catch "out of memory" };
        },
    }
}

fn toolRender(ctx: *anyopaque, a: std.mem.Allocator, view: []const u8, mode: []const u8) tools.RenderOutcome {
    const m: *Model = @ptrCast(@alignCast(ctx));
    if (!m.doc.hasDoc()) return .{ .ok = false, .caption = "no document is loaded yet." };
    const sheet = std.mem.eql(u8, mode, "sheet");
    const res = if (sheet) blk: {
        const eng = @import("engine.zig");
        const req = (eng.Request{ .doc = m.doc.doc, .view = view }).build(gpa) catch return .{ .ok = false, .caption = "out of memory" };
        defer gpa.free(req);
        break :blk m.doc.engine.call(gpa, "sheet_drawing", req);
    } else m.doc.drawingJson(view);
    switch (res) {
        .err => |e| {
            defer gpa.free(e);
            return .{ .ok = false, .caption = std.fmt.allocPrint(a, "render failed: {s}", .{flow.engineMessage(e)}) catch "render failed" };
        },
        .ok => |json| {
            defer gpa.free(json);
            var d = draw.ir.parse(gpa, json) catch return .{ .ok = false, .caption = "the engine's drawing could not be parsed" };
            defer d.deinit();
            const png = draw.renderPng(gpa, &d, .{ .width_px = 1400 }) catch return .{ .ok = false, .caption = "rasterizing failed" };
            defer gpa.free(png);
            const info = m.doc.info.?;
            var scale: []const u8 = "";
            var notes: usize = 0;
            for (info.views) |v| if (std.mem.eql(u8, v.id, view)) {
                scale = v.scale;
                notes = v.notes.len;
            };
            const caption = std.fmt.allocPrint(a, "{s} {s} rendered at {s}; {d} notes; {d} errors, {d} warnings", .{
                if (sheet) "sheet for view" else "view",
                view,
                if (scale.len > 0) scale else "NTS",
                notes,
                m.doc.errorCount(),
                m.doc.warnCount(),
            }) catch "rendered";
            return .{ .ok = true, .png = a.dupe(u8, png) catch return .{ .ok = false, .caption = "out of memory" }, .caption = caption };
        },
    }
}
