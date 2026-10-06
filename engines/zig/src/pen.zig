//! The pens and layer keys the engine draws with (SPEC 7). A style may re-weight these pens and rename their layers, but the
//! engine only ever emits these names, so they are enums: a typo is a compile error and every `switch` over them is exhaustive.

/// Pen of a drawing item. The tag names are the pen names in the style JSON and in the drawing IR.
pub const Pen = enum {
    cut,
    profile,
    beyond,
    hidden,
    hatch,
    rebar,
    steel,
    membrane,
    vapor,
    anno,
    dim,
    @"break",
    title,
    frame,

    pub fn name(self: Pen) []const u8 {
        return @tagName(self);
    }

    /// Which style layer an item drawn with this pen belongs to.
    pub fn layer(self: Pen) LayerKey {
        return switch (self) {
            .cut, .profile, .membrane, .vapor => .cut,
            .beyond => .beyond,
            .hidden => .hidden,
            .hatch => .hatch,
            .rebar => .rebar,
            .steel => .steel,
            .anno => .notes,
            .dim => .dims,
            .@"break" => .@"break",
            .title, .frame => .title,
        };
    }
};

/// Key of a style layer (`layers.<key>` in the style JSON).
pub const LayerKey = enum {
    cut,
    beyond,
    hidden,
    hatch,
    steel,
    rebar,
    notes,
    dims,
    title,
    @"break",

    pub fn key(self: LayerKey) []const u8 {
        return @tagName(self);
    }
};

/// Layers in the order a drawing lists them (the order exporters stack them).
pub const draw_order = [_]LayerKey{ .hatch, .beyond, .cut, .steel, .rebar, .hidden, .@"break", .notes, .dims, .title };

const std = @import("std");

test "every pen has a layer and every layer is drawn" {
    inline for (std.meta.tags(Pen)) |p| _ = p.layer();
    try std.testing.expectEqual(std.meta.tags(LayerKey).len, draw_order.len);
}
