//! Component builders (SPEC 5): the registry. Every component type lives in its own file, `builders/<type>.zig`, that
//! declares
//!   - `Params`: a struct of the type's parameters (keys, defaults, choices, ranges and catalog text: see params.zig); the
//!     parser, `kerf catalog` / `kerf schema <type>`, the accepted-key list and the canonical key order all come from it;
//!   - `build(ctx)`: turn the parsed parameters into a `Built` (prisms, anchors, zones) in local coordinates, reporting problems as
//!     E_PARAM diagnostics and returning null when the component cannot be built.
//! Helpers shared by the builders are in `builders/common.zig`.
//!
//! Adding a component type: (1) a tag in `catalog.Type` and an entry in `catalog.entries` (prose, parts, anchors, example, traits)
//! at the same position; (2) `builders/<type>.zig` with `Params` and `build`; (3) the import, the `build` arm and the
//! `param_structs` entry below. Steps (1) and (3) are exhaustive, so the compiler names whatever is missing.

const std = @import("std");
const catalog = @import("catalog.zig");
const params_mod = @import("params.zig");
const model = @import("model.zig");
const common = @import("builders/common.zig");

pub const lumber = @import("builders/lumber.zig");
pub const panel = @import("builders/panel.zig");
pub const cmu_wall = @import("builders/cmu_wall.zig");
pub const concrete = @import("builders/concrete.zig");
pub const rebar = @import("builders/rebar.zig");
pub const anchor_bolt = @import("builders/anchor_bolt.zig");
pub const connector = @import("builders/connector.zig");
pub const truss = @import("builders/truss.zig");
pub const membrane = @import("builders/membrane.zig");
pub const fill = @import("builders/fill.zig");
pub const insulation = @import("builders/insulation.zig");
pub const solid = @import("builders/solid.zig");
pub const flashing = @import("builders/flashing.zig");
pub const joint = @import("builders/joint.zig");

pub const BuildError = common.BuildError;
pub const Ctx = common.Ctx;
pub const Built = model.Built;
pub const mirrorAboutCenter = common.mirrorAboutCenter;
pub const worldBox = common.worldBox;

pub fn build(ctx: *Ctx) BuildError!?Built {
    return switch (ctx.comp.ty.type) {
        .lumber => lumber.build(ctx),
        .panel => panel.build(ctx),
        .cmu_wall => cmu_wall.build(ctx),
        .concrete => concrete.build(ctx),
        .rebar => rebar.build(ctx),
        .anchor_bolt => anchor_bolt.build(ctx),
        .connector => connector.build(ctx),
        .truss => truss.build(ctx),
        .membrane => membrane.build(ctx),
        .fill => fill.build(ctx),
        .insulation => insulation.build(ctx),
        .solid => solid.build(ctx),
        .flashing => flashing.build(ctx),
        .joint => joint.build(ctx),
    };
}

/// The parameter struct of every component type, in `catalog.Type` order.
pub const param_structs = .{ lumber.Params, panel.Params, cmu_wall.Params, concrete.Params, rebar.Params, anchor_bolt.Params, connector.Params, truss.Params, membrane.Params, fill.Params, insulation.Params, solid.Params, flashing.Params, joint.Params };

comptime {
    if (param_structs.len != std.meta.tags(catalog.Type).len) @compileError("builders.param_structs must have one struct per catalog.Type tag");
}

test "every choice of an enum parameter is mentioned in its catalog row (docs cannot drift from the parser)" {
    inline for (param_structs) |S| {
        const rows = params_mod.rows(S);
        const info = @typeInfo(S).@"struct";
        inline for (info.field_names, info.field_types) |field_name, field_type| {
            const Base = switch (@typeInfo(field_type)) {
                .optional => |o| o.child,
                else => field_type,
            };
            if (@typeInfo(Base) == .@"enum") {
                var found_row = false;
                for (rows) |r| {
                    if (!std.mem.eql(u8, r.names[0], field_name)) continue;
                    found_row = true;
                    inline for (@typeInfo(Base).@"enum".field_names) |choice| {
                        if (std.mem.indexOf(u8, r.desc, choice) == null and std.mem.indexOf(u8, r.def, choice) == null) {
                            std.debug.print("{s}.{s}: choice '{s}' is not in its catalog text\n", .{ @typeName(S), field_name, choice });
                            return error.TestUnexpectedResult;
                        }
                    }
                }
                try std.testing.expect(found_row);
            }
        }
    }
}
