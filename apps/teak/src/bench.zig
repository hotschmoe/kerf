//! Native micro-benchmark of the app's engine-facing pipeline (run with
//! `zig build bench`, ReleaseFast): what a user waits for after opening a
//! sample, panning, or when Claude calls kerf_render.

const std = @import("std");
const kerf = @import("kerf");
const draw = @import("draw/mod.zig");
const session = @import("app/session.zig");
const engine_real = @import("app/engine_real.zig");
const scene3d = @import("app/scene3d.zig");

const gpa = std.heap.c_allocator;

fn now() std.Io.Timestamp {
    return std.Io.Clock.awake.now(std.Options.debug_io);
}

fn ms(a: std.Io.Timestamp, b: std.Io.Timestamp) f64 {
    return @as(f64, @floatFromInt(b.nanoseconds - a.nanoseconds)) / 1.0e6;
}

pub fn main() !void {
    const samples = .{
        .{ "truss-bearing-cmu", @embedFile("sample_truss_json") },
        .{ "monopour-slab-door-recess", @embedFile("sample_slab_json") },
        .{ "flush-beam-strap", @embedFile("sample_strap_json") },
    };
    var out_buf: [4096]u8 = undefined;
    var w = std.Io.File.stdout().writer(std.Options.debug_io, &out_buf);
    const o = &w.interface;
    try o.print("{s:<28} {s:>8} {s:>9} {s:>8} {s:>8} {s:>10} {s:>8} {s:>10}\n", .{ "sample", "load ms", "drawing", "parse", "tess", "render1400", "mesh", "export pdf" });
    inline for (samples) |s| {
        var sess = session.Session.init(gpa, engine_real.engine(), "");
        var err: ?[]u8 = null;
        const t0 = now();
        _ = try sess.load(s[1], &err);
        const t1 = now();
        const view_id = sess.info.?.views[0].id;
        const dj = sess.drawingJson(view_id).ok;
        const t2 = now();
        var d = try draw.ir.parse(gpa, dj);
        const t3 = now();
        var tess = draw.Tessellator.init(gpa);
        var font = try draw.Font.initEmbedded(gpa);
        const view = draw.View.fit(d.bounds, 760, 800, 28);
        var i: usize = 0;
        while (i < 20) : (i += 1) try tess.build(&d, &font, view, draw.Palette.live(), .{});
        const t4 = now();
        const png = try draw.render.renderPngWithFont(gpa, &d, &font, .{ .width_px = 1400 });
        const t5 = now();
        const mj = sess.meshJson().ok;
        var mesh = try draw.mesh.parse(gpa, mj);
        var built = try scene3d.build(gpa, &mesh, null);
        const t6 = now();
        const pdf = sess.exportBytes(view_id, "pdf", true).ok;
        const t7 = now();
        try o.print("{s:<28} {d:>8.1} {d:>9.1} {d:>8.1} {d:>8.2} {d:>10.1} {d:>8.1} {d:>10.1}   png={d}KB tris={d} pdf={d}KB\n", .{ s[0], ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, t4) / 20.0, ms(t4, t5), ms(t5, t6), ms(t6, t7), png.len / 1024, tess.verts().len / 3, pdf.len / 1024 });
        built.deinit();
    }
    try o.flush();
}
