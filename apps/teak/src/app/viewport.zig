//! 2D viewport: owns the parsed Drawing IR of the active view, the camera,
//! hover/selection, note dragging, and the tessellated triangle list that the
//! UI hands to a teak canvas. Pure logic over `draw/`; driven by the app with
//! teak `CanvasEvent`s (declared locally so this file stays testable without
//! the UI framework).

const std = @import("std");
const Allocator = std.mem.Allocator;
const draw = @import("../draw/mod.zig");
const ident = @import("ident.zig");

const MaybeId = ident.MaybeId;

pub const EventKind = enum { down, move, up, wheel, leave, layout };
pub const Button = enum { none, left, middle, right };

/// The subset of teak.CanvasEvent the viewport consumes.
pub const Event = struct {
    kind: EventKind,
    x: f32 = 0,
    y: f32 = 0,
    dx: f32 = 0,
    dy: f32 = 0,
    button: Button = .none,
    ctrl: bool = false,
    shift: bool = false,
    w: f32 = 0,
    h: f32 = 0,
};

/// What the app must do after an event.
pub const Out = struct {
    /// The triangle list changed (bump `key`, re-upload).
    redraw: bool = false,
    /// The selection changed to this (empty = cleared).
    select: ?MaybeId = null,
    /// A note drag finished: commit `place` for this annotation id.
    commit_note: ?struct { id: ident.Id, place: [2]f64 } = null,
};

const Drag = union(enum) {
    none,
    pan,
    /// Pending click on a drawing item: becomes a select on `up` unless the
    /// pointer moved; for notes it becomes a drag after a few px.
    press: struct { src: MaybeId, is_note: bool, x: f32, y: f32 },
    note: struct {
        id: ident.Id,
        start: [2]f64, // model coords at drag start
        place0: [2]f64, // top-left of first text line at drag start
        text_dx: f64 = 0,
        text_dy: f64 = 0,
    },
};

pub const Vp = struct {
    gpa: Allocator,
    font: draw.Font,
    tess: draw.Tessellator,
    palette: draw.Palette = draw.Palette.live(),
    drawing: ?draw.ir.Drawing = null,
    view: draw.View = .{ .px_per_model_in = 8, .origin_x = 0, .origin_y = 0, .width = 100, .height = 100 },
    fitted: bool = false,
    w: f32 = 0,
    h: f32 = 0,
    /// Content revision for the canvas frame-diff.
    key: u64 = 1,
    hover: MaybeId = .{},
    sel: MaybeId = .{},
    grid: bool = true,
    page: bool = false,
    drag: Drag = .none,
    cursor: ?[2]f64 = null,
    /// Names of annotation ids of kind "note" in this view (drag eligible).
    /// Owned by the caller; borrowed here.
    note_ids: []const []const u8 = &.{},

    pub fn init(gpa: Allocator) !*Vp {
        const self = try gpa.create(Vp);
        errdefer gpa.destroy(self);
        self.* = .{ .gpa = gpa, .font = try draw.Font.initEmbedded(gpa), .tess = draw.Tessellator.init(gpa) };
        return self;
    }

    pub fn deinit(self: *Vp) void {
        if (self.drawing) |*d| d.deinit();
        self.tess.deinit();
        self.font.deinit();
        self.gpa.destroy(self);
    }

    /// Replace the drawing from engine JSON. Keeps the camera unless `refit`.
    pub fn setDrawing(self: *Vp, json: []const u8, refit: bool) !void {
        var d = try draw.ir.parse(self.gpa, json);
        errdefer d.deinit();
        if (self.drawing) |*old| old.deinit();
        self.drawing = d;
        if (refit) self.fitted = false;
        self.drag = .none;
        if (self.w > 0 and !self.fitted) self.fit();
        try self.retess();
    }

    pub fn clearDrawing(self: *Vp) void {
        if (self.drawing) |*old| old.deinit();
        self.drawing = null;
        self.tess.buf.clear();
        self.key +%= 1;
    }

    pub fn fit(self: *Vp) void {
        const d = self.drawing orelse return;
        self.view = draw.View.fit(d.bounds, self.w, self.h, 28);
        self.fitted = true;
    }

    pub fn zoomStep(self: *Vp, factor: f32) void {
        self.view = self.view.zoomAt(self.w * 0.5, self.h * 0.5, factor);
        self.clampZoom();
    }

    fn clampZoom(self: *Vp) void {
        const s = std.math.clamp(self.view.px_per_model_in, 0.4, 600);
        if (s != self.view.px_per_model_in) {
            const f = s / self.view.px_per_model_in;
            self.view = self.view.zoomAt(self.w * 0.5, self.h * 0.5, f);
        }
    }

    pub fn retess(self: *Vp) !void {
        const d = &(self.drawing orelse return);
        self.view.width = self.w;
        self.view.height = self.h;
        try self.tess.build(d, &self.font, self.view, self.palette, .{
            .selected = self.sel.get(),
            .hovered = self.hover.get(),
            .grid = self.grid,
            .page = self.page,
        });
        self.key +%= 1;
    }

    pub fn verts(self: *const Vp) []const draw.tess.Vert {
        return self.tess.verts();
    }

    pub fn setSelection(self: *Vp, s: MaybeId) !void {
        self.sel = s;
        try self.retess();
    }

    fn modelAt(self: *const Vp, x: f32, y: f32) [2]f64 {
        return .{ self.view.modelX(x), self.view.modelY(y) };
    }

    fn pickAt(self: *Vp, x: f32, y: f32) MaybeId {
        const d = &(self.drawing orelse return .{});
        const tol = 4.0 / @as(f64, self.view.px_per_model_in);
        const m = self.modelAt(x, y);
        if (draw.pick.pick(d, &self.font, m[0], m[1], tol)) |src| return MaybeId.of(draw.ir.srcBase(src));
        return .{};
    }

    fn isNote(self: *const Vp, id: []const u8) bool {
        for (self.note_ids) |n| if (std.mem.eql(u8, n, id)) return true;
        return false;
    }

    pub fn handle(self: *Vp, ev: Event) !Out {
        var out: Out = .{};
        switch (ev.kind) {
            .layout => {
                const changed = ev.w != self.w or ev.h != self.h;
                self.w = ev.w;
                self.h = ev.h;
                if (changed) {
                    if (!self.fitted) self.fit();
                    try self.retess();
                    out.redraw = true;
                }
            },
            .wheel => {
                // DOM sign: positive dy = scroll down = zoom out. Pinch (ctrl) is stronger.
                const k: f32 = if (ev.ctrl) 0.01 else 0.0015;
                const f = @exp(-ev.dy * k);
                self.view = self.view.zoomAt(ev.x, ev.y, f);
                self.clampZoom();
                self.fitted = true;
                try self.retess();
                out.redraw = true;
            },
            .leave => {
                if (self.hover.set) {
                    self.hover = .{};
                    try self.retess();
                    out.redraw = true;
                }
                self.cursor = null;
            },
            .down => {
                if (ev.button == .middle or ev.button == .right) {
                    self.drag = .pan;
                } else if (ev.button == .left) {
                    const hit = self.pickAt(ev.x, ev.y);
                    if (hit.set) {
                        self.drag = .{ .press = .{ .src = hit, .is_note = self.isNote(hit.id.slice()), .x = ev.x, .y = ev.y } };
                    } else {
                        self.drag = .pan;
                        // Click on empty space clears the selection.
                        if (self.sel.set) {
                            self.sel = .{};
                            out.select = .{};
                            try self.retess();
                            out.redraw = true;
                        }
                    }
                }
            },
            .move => {
                self.cursor = self.modelAt(ev.x, ev.y);
                switch (self.drag) {
                    .pan => {
                        self.view = self.view.panned(ev.dx, ev.dy);
                        self.fitted = true;
                        try self.retess();
                        out.redraw = true;
                    },
                    .press => |p| {
                        const moved = @abs(ev.x - p.x) > 3 or @abs(ev.y - p.y) > 3;
                        if (moved and p.is_note) {
                            if (self.drawing) |*d| {
                                if (draw.pick.textOrigin(d, p.src.id.slice())) |o| {
                                    const m = self.modelAt(p.x, p.y);
                                    self.drag = .{ .note = .{
                                        .id = p.src.id,
                                        .start = m,
                                        // place = top-left of the first text line (cap height above baseline)
                                        .place0 = .{ o.x, o.y + o.h },
                                    } };
                                    return self.handleNoteMove(ev, &out);
                                }
                            }
                            self.drag = .pan;
                        } else if (moved) {
                            self.drag = .pan;
                            self.view = self.view.panned(ev.dx, ev.dy);
                            self.fitted = true;
                            try self.retess();
                            out.redraw = true;
                        }
                    },
                    .note => return self.handleNoteMove(ev, &out),
                    .none => {
                        const hit = self.pickAt(ev.x, ev.y);
                        if (!hit.same(&self.hover)) {
                            self.hover = hit;
                            try self.retess();
                            out.redraw = true;
                        }
                    },
                }
            },
            .up => {
                switch (self.drag) {
                    .press => |p| {
                        // A plain click: select.
                        self.sel = p.src;
                        out.select = p.src;
                        try self.retess();
                        out.redraw = true;
                    },
                    .note => |n| {
                        const m = self.modelAt(ev.x, ev.y);
                        const dx = m[0] - n.start[0];
                        const dy = m[1] - n.start[1];
                        if (@abs(dx) > 1e-6 or @abs(dy) > 1e-6) {
                            out.commit_note = .{ .id = n.id, .place = .{ n.place0[0] + dx, n.place0[1] + dy } };
                        }
                        // The app re-fetches the drawing after the op; drop the preview offset now.
                        self.sel = MaybeId.of(n.id.slice());
                        out.select = self.sel;
                    },
                    else => {},
                }
                self.drag = .none;
            },
        }
        return out;
    }

    /// Live preview of a note drag: translate the note's own text items
    /// (the leader is re-routed by the engine once the op is applied).
    fn handleNoteMove(self: *Vp, ev: Event, out: *Out) !Out {
        const n = &self.drag.note;
        const d = &(self.drawing orelse return out.*);
        const m = self.modelAt(ev.x, ev.y);
        const want_dx = m[0] - n.start[0];
        const want_dy = m[1] - n.start[1];
        const step_x = want_dx - n.text_dx;
        const step_y = want_dy - n.text_dy;
        const items: []draw.ir.Item = @constCast(d.items);
        for (items) |*it| {
            if (!draw.ir.srcMatches(it.src, n.id.slice())) continue;
            switch (it.body) {
                .text => |*t| {
                    t.x += step_x;
                    t.y += step_y;
                },
                else => {},
            }
        }
        n.text_dx = want_dx;
        n.text_dy = want_dy;
        try self.retess();
        out.redraw = true;
        return out.*;
    }
};

test "load fixture, fit, pick and select" {
    const a = std.testing.allocator;
    const json = @embedFile("fixture_truss_drawing");
    var vp = try Vp.init(a);
    defer vp.deinit();
    _ = try vp.handle(.{ .kind = .layout, .w = 900, .h = 700 });
    try vp.setDrawing(json, true);
    try std.testing.expect(vp.fitted);
    try std.testing.expect(vp.verts().len > 300);
    // Hover/pick something near the middle of the drawing.
    const d = vp.drawing.?;
    const cx = (d.bounds[0] + d.bounds[2]) * 0.5;
    const cy = (d.bounds[1] + d.bounds[3]) * 0.5;
    const sx = vp.view.sx(cx);
    const sy = vp.view.sy(cy);
    _ = try vp.handle(.{ .kind = .move, .x = sx, .y = sy });
    _ = try vp.handle(.{ .kind = .down, .button = .left, .x = sx, .y = sy });
    const o = try vp.handle(.{ .kind = .up, .button = .left, .x = sx, .y = sy });
    _ = o;
}

test "wheel zoom keeps the point under the cursor" {
    const a = std.testing.allocator;
    var vp = try Vp.init(a);
    defer vp.deinit();
    try vp.setDrawing(@embedFile("fixture_truss_drawing"), true);
    _ = try vp.handle(.{ .kind = .layout, .w = 800, .h = 600 });
    const before = vp.modelAt(300, 200);
    _ = try vp.handle(.{ .kind = .wheel, .x = 300, .y = 200, .dy = -240 });
    const after = vp.modelAt(300, 200);
    try std.testing.expectApproxEqAbs(before[0], after[0], 1e-3);
    try std.testing.expectApproxEqAbs(before[1], after[1], 1e-3);
}
