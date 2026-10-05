//! Drawing IR (SPEC 10): the model-space output of a view, input of every exporter.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const style_mod = @import("style.zig");
const Allocator = std.mem.Allocator;
const Pt = geom.Pt;
const V2 = geom.V2;

pub const Align = enum { left, center, right };
pub const VAlign = enum { baseline, middle, top };

pub const PathItem = struct {
    layer: []const u8,
    pen: []const u8,
    src: []const u8,
    closed: bool,
    pts: []const Pt,
};

pub const FillItem = struct {
    layer: []const u8,
    src: []const u8,
    loops: []const []const Pt,
};

pub const HatchItem = struct {
    layer: []const u8,
    pen: []const u8,
    src: []const u8,
    pattern: []const u8,
    scale: f64,
    angle: f64,
    loops: []const []const Pt,
    lines: []const [4]f64,
};

pub const TextItem = struct {
    layer: []const u8,
    pen: []const u8,
    src: []const u8,
    s: []const u8,
    x: f64,
    y: f64,
    h: f64,
    rot: f64 = 0,
    align_: Align = .left,
    valign: VAlign = .baseline,
};

pub const Item = union(enum) {
    path: PathItem,
    fill: FillItem,
    hatch: HatchItem,
    text: TextItem,

    pub fn src(self: Item) []const u8 {
        return switch (self) {
            inline else => |x| x.src,
        };
    }
    pub fn layer(self: Item) []const u8 {
        return switch (self) {
            inline else => |x| x.layer,
        };
    }
};

pub const LayerDef = struct {
    name: []const u8,
    lineweight_mm: f64,
    linetype: []const u8 = "CONTINUOUS",
};

pub const Drawing = struct {
    doc: []const u8,
    view: []const u8,
    kind: []const u8,
    scale: f64,
    bounds: [4]f64,
    items: []const Item,
    layers: []const LayerDef,
    diagnostics: []const model.Diag,
    style: *const style_mod.Style,

    // --- exporter extras (not part of the JSON IR) ---
    number: []const u8 = "",
    title: []const u8 = "",
    scale_label: []const u8 = "",
    sheet_no: []const u8 = "",
    date: []const u8 = "",
    code_basis: []const u8 = "",
    has_unverified: bool = false,
    /// The model-space crop window (section) or the projected window (iso).
    crop: geom.Box = .{},
    /// Content extents excluding title items.
    detail_bounds: geom.Box = .{},
};

// ---- JSON output ----------------------------------------------------------------------------------------

fn num(out: *std.ArrayList(u8), a: Allocator, x: f64) Allocator.Error!void {
    try json.writeNumber(out, a, x);
}

fn str(out: *std.ArrayList(u8), a: Allocator, s: []const u8) Allocator.Error!void {
    try json.writeString(out, a, s);
}

fn writePts(out: *std.ArrayList(u8), a: Allocator, pts: []const Pt) Allocator.Error!void {
    try out.append(a, '[');
    for (pts, 0..) |p, i| {
        if (i > 0) try out.append(a, ',');
        try out.append(a, '[');
        try num(out, a, p.x);
        try out.append(a, ',');
        try num(out, a, p.y);
        if (p.b != 0) {
            try out.append(a, ',');
            // bulges keep extra precision
            var buf: [48]u8 = undefined;
            const t = std.fmt.bufPrint(&buf, "{d}", .{@round(p.b * 1e6) / 1e6}) catch "0";
            try out.appendSlice(a, t);
        }
        try out.append(a, ']');
    }
    try out.append(a, ']');
}

fn writeLoops(out: *std.ArrayList(u8), a: Allocator, loops: []const []const Pt) Allocator.Error!void {
    try out.append(a, '[');
    for (loops, 0..) |l, i| {
        if (i > 0) try out.append(a, ',');
        try writePts(out, a, l);
    }
    try out.append(a, ']');
}

pub fn toJson(a: Allocator, d: *const Drawing) Allocator.Error![]u8 {
    var out: std.ArrayList(u8) = .empty;
    try out.appendSlice(a, "{\"kerf_drawing\":\"0.1\",\"doc\":");
    try str(&out, a, d.doc);
    try out.appendSlice(a, ",\"view\":");
    try str(&out, a, d.view);
    try out.appendSlice(a, ",\"kind\":");
    try str(&out, a, d.kind);
    try out.appendSlice(a, ",\"scale\":");
    try num(&out, a, d.scale);
    try out.appendSlice(a, ",\"bounds\":[");
    for (d.bounds, 0..) |b, i| {
        if (i > 0) try out.append(a, ',');
        try num(&out, a, b);
    }
    try out.appendSlice(a, "],\n\"pens\":{");
    for (d.style.pens, 0..) |p, i| {
        if (i > 0) try out.append(a, ',');
        try str(&out, a, p.name);
        try out.appendSlice(a, ":{\"width_mm\":");
        try num(&out, a, p.width_mm);
        try out.appendSlice(a, ",\"dash_mm\":");
        if (p.dash_mm) |dm| {
            try out.append(a, '[');
            for (dm, 0..) |x, k| {
                if (k > 0) try out.append(a, ',');
                try num(&out, a, x);
            }
            try out.append(a, ']');
        } else try out.appendSlice(a, "null");
        try out.append(a, '}');
    }
    try out.appendSlice(a, "},\n\"layers\":[");
    for (d.layers, 0..) |l, i| {
        if (i > 0) try out.append(a, ',');
        try out.appendSlice(a, "{\"name\":");
        try str(&out, a, l.name);
        try out.appendSlice(a, ",\"lineweight_mm\":");
        try num(&out, a, l.lineweight_mm);
        try out.append(a, '}');
    }
    try out.appendSlice(a, "],\n\"items\":[\n");
    for (d.items, 0..) |it, i| {
        if (i > 0) try out.appendSlice(a, ",\n");
        switch (it) {
            .path => |p| {
                try out.appendSlice(a, "{\"t\":\"path\",\"layer\":");
                try str(&out, a, p.layer);
                try out.appendSlice(a, ",\"pen\":");
                try str(&out, a, p.pen);
                try out.appendSlice(a, ",\"src\":");
                try str(&out, a, p.src);
                try out.appendSlice(a, if (p.closed) ",\"closed\":true,\"pts\":" else ",\"closed\":false,\"pts\":");
                try writePts(&out, a, p.pts);
                try out.append(a, '}');
            },
            .fill => |f| {
                try out.appendSlice(a, "{\"t\":\"fill\",\"layer\":");
                try str(&out, a, f.layer);
                try out.appendSlice(a, ",\"src\":");
                try str(&out, a, f.src);
                try out.appendSlice(a, ",\"loops\":");
                try writeLoops(&out, a, f.loops);
                try out.append(a, '}');
            },
            .hatch => |h| {
                try out.appendSlice(a, "{\"t\":\"hatch\",\"layer\":");
                try str(&out, a, h.layer);
                try out.appendSlice(a, ",\"pen\":");
                try str(&out, a, h.pen);
                try out.appendSlice(a, ",\"src\":");
                try str(&out, a, h.src);
                try out.appendSlice(a, ",\"pattern\":");
                try str(&out, a, h.pattern);
                try out.appendSlice(a, ",\"scale\":");
                try num(&out, a, h.scale);
                try out.appendSlice(a, ",\"angle\":");
                try num(&out, a, h.angle);
                try out.appendSlice(a, ",\"loops\":");
                try writeLoops(&out, a, h.loops);
                try out.appendSlice(a, ",\"lines\":[");
                for (h.lines, 0..) |ln, k| {
                    if (k > 0) try out.append(a, ',');
                    try out.append(a, '[');
                    for (ln, 0..) |v, q| {
                        if (q > 0) try out.append(a, ',');
                        try num(&out, a, v);
                    }
                    try out.append(a, ']');
                }
                try out.appendSlice(a, "]}");
            },
            .text => |t| {
                try out.appendSlice(a, "{\"t\":\"text\",\"layer\":");
                try str(&out, a, t.layer);
                try out.appendSlice(a, ",\"pen\":");
                try str(&out, a, t.pen);
                try out.appendSlice(a, ",\"src\":");
                try str(&out, a, t.src);
                try out.appendSlice(a, ",\"s\":");
                try str(&out, a, t.s);
                try out.appendSlice(a, ",\"x\":");
                try num(&out, a, t.x);
                try out.appendSlice(a, ",\"y\":");
                try num(&out, a, t.y);
                try out.appendSlice(a, ",\"h\":");
                try num(&out, a, t.h);
                try out.appendSlice(a, ",\"rot\":");
                try num(&out, a, t.rot);
                try out.appendSlice(a, ",\"align\":\"");
                try out.appendSlice(a, @tagName(t.align_));
                try out.appendSlice(a, "\",\"valign\":\"");
                try out.appendSlice(a, @tagName(t.valign));
                try out.appendSlice(a, "\"}");
            },
        }
    }
    try out.appendSlice(a, "\n],\n\"diagnostics\":[");
    for (d.diagnostics, 0..) |dg, i| {
        if (i > 0) try out.append(a, ',');
        const v = try model.diagToJson(a, dg);
        try json.writeCompact(&out, a, v);
    }
    try out.appendSlice(a, "]}\n");
    return out.items;
}
