//! End-to-end UI tests without a window: Model -> view -> layout -> snapshot.

const std = @import("std");
const teak = @import("teak");
const model = @import("model.zig");
const update = @import("update.zig").update;
const view = @import("view.zig").view;
const flow = @import("docflow.zig");
const th = @import("theme.zig");

const Model = model.Model;
const Msg = model.Msg;

fn frame(m: *const Model, cb: *teak.CmdBuffer(Msg), rects: *std.ArrayList(teak.Rect), w: f32, h: f32) !void {
    cb.reset();
    cb.theme = th.theme;
    view(m, cb);
    try std.testing.expect(teak.validateBalance(cb.cmds.items) == null);
    try rects.resize(std.testing.allocator, cb.cmds.items.len);
    teak.LayoutEngine.doLayout(rects.items, cb.cmds.items, w, h, teak.monoMeasurer());
}

test "empty app builds a balanced view" {
    var m = Model.init();
    var cb = teak.CmdBuffer(Msg).init(std.testing.allocator);
    defer cb.deinit();
    var rects: std.ArrayList(teak.Rect) = .empty;
    defer rects.deinit(std.testing.allocator);
    try frame(&m, &cb, &rects, 1440, 900);
    try std.testing.expect(cb.cmds.items.len > 30);
}

test "load a sample, select a part, edit a note, undo" {
    var m = Model.init();
    var cb = teak.CmdBuffer(Msg).init(std.testing.allocator);
    defer cb.deinit();
    var rects: std.ArrayList(teak.Rect) = .empty;
    defer rects.deinit(std.testing.allocator);

    update(&m, .{ .load_sample = 0 });
    try std.testing.expect(m.ready);
    try std.testing.expect(m.doc.info.?.components.len > 5);
    try frame(&m, &cb, &rects, 1440, 900);

    update(&m, .{ .select = @import("ident.zig").Id.from("sill_plate") });
    try std.testing.expect(m.sel.set);
    update(&m, .{ .insp_tab = .notes });
    update(&m, .{ .select_note = 0 });
    try std.testing.expect(m.note_sel == 0);
    try frame(&m, &cb, &rects, 1440, 900);

    // Edit the note text.
    m.note_ed.set("2X8 TEST NOTE");
    update(&m, .note_apply);
    try std.testing.expectEqual(@as(u32, 1), m.doc.rev);
    try std.testing.expectEqualStrings("2X8 TEST NOTE", m.doc.info.?.views[0].notes[0].text);
    update(&m, .undo);
    try std.testing.expectEqual(@as(u32, 0), m.doc.rev);
    try frame(&m, &cb, &rects, 1440, 900);
}

test "every tab of every sample builds" {
    var m = Model.init();
    var cb = teak.CmdBuffer(Msg).init(std.testing.allocator);
    defer cb.deinit();
    var rects: std.ArrayList(teak.Rect) = .empty;
    defer rects.deinit(std.testing.allocator);
    for (0..model.samples.len) |s| {
        update(&m, .{ .load_sample = @intCast(s) });
        try std.testing.expect(m.ready);
        const n = m.nViews() + 2;
        for (0..n) |t| {
            update(&m, .{ .select_tab = @intCast(t) });
            for ([_]model.InspTab{ .parts, .notes, .diff, .diag }) |it| {
                update(&m, .{ .insp_tab = it });
                try frame(&m, &cb, &rects, 1440, 900);
            }
        }
    }
}

test "demo mode drives the whole chat loop" {
    var m = Model.init();
    var cb = teak.CmdBuffer(Msg).init(std.testing.allocator);
    defer cb.deinit();
    var rects: std.ArrayList(teak.Rect) = .empty;
    defer rects.deinit(std.testing.allocator);
    update(&m, .toggle_demo);
    update(&m, .{ .focus = .chat });
    m.chat_ed.set("Prefab truss bearing on an 8 inch CMU wall please");
    // The demo has no key; the session refuses without one, so give it a dummy.
    @import("chatglue.zig").setKey(&m, "sk-ant-demo-key-0000000000");
    update(&m, .chat_send);
    var guard: u32 = 0;
    while (guard < 40 and (m.chat.session.isBusy() or m.demo_pending != null)) : (guard += 1) update(&m, .tick);
    try std.testing.expect(!m.chat.session.isBusy());
    try std.testing.expect(m.ready);
    try std.testing.expect(m.doc.log.items.len >= 1);
    try frame(&m, &cb, &rects, 1440, 900);
}
