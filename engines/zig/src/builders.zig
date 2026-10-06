//! Component builders (SPEC 5): turn a component's JSON params into a `Built` (prisms, anchors,
//! zones) in local coordinates. Builders validate their params and report E_PARAM diagnostics;
//! they return null when the component cannot be built.

const std = @import("std");
const json = @import("json.zig");
const geom = @import("geom.zig");
const model = @import("model.zig");
const units = @import("units.zig");
const cast = @import("num.zig");
const limits = @import("limits.zig");
const catalog = @import("catalog.zig");
const params_mod = @import("params.zig");
const scene_mod = @import("scene.zig");
const style_mod = @import("style.zig");
const path_geom = @import("pathgeom.zig");
const Allocator = std.mem.Allocator;
const V2 = geom.V2;
const Pt = geom.Pt;
const Built = model.Built;
const Prism = model.Prism;
const Params = model.Params;
const Box = geom.Box;
const common = @import("builders/common.zig");
pub const joint = @import("builders/joint.zig");
pub const flashing = @import("builders/flashing.zig");
pub const solid = @import("builders/solid.zig");
pub const insulation = @import("builders/insulation.zig");
pub const fill = @import("builders/fill.zig");
pub const membrane = @import("builders/membrane.zig");
pub const truss = @import("builders/truss.zig");
pub const connector = @import("builders/connector.zig");
pub const anchor_bolt = @import("builders/anchor_bolt.zig");
pub const rebar = @import("builders/rebar.zig");
pub const concrete = @import("builders/concrete.zig");
pub const cmu_wall = @import("builders/cmu_wall.zig");
pub const panel = @import("builders/panel.zig");
pub const lumber = @import("builders/lumber.zig");
pub const BuildError = common.BuildError;
pub const Ctx = common.Ctx;
pub const mirrorAboutCenter = common.mirrorAboutCenter;
pub const worldBox = common.worldBox;
const boxOfPrisms = common.boxOfPrisms;
const zoneRect = common.zoneRect;
const onePrism = common.onePrism;
const ftin = common.ftin;
const fmtNum = common.fmtNum;
const quadOf = common.quadOf;
const parseSawn = common.parseSawn;
const parseActual = common.parseActual;
const parsePointList = common.parsePointList;
const dropDuplicatePoints = common.dropDuplicatePoints;
const orientedCcw = common.orientedCcw;
const materialOk = common.materialOk;
const lengthOrUntil = common.lengthOrUntil;
const parseCover = common.parseCover;
const mirrorBuilt = common.mirrorBuilt;
const gaugeThickness = common.gaugeThickness;
const vsOf = common.vsOf;
const pathLen = common.pathLen;
const materialOr = common.materialOr;

// ---- helpers ---------------------------------------------------------------------------------------

// ---- lumber -------------------------------------------------------------------------------------------

// ---- panel -----------------------------------------------------------------------------------------------

// ---- cover parsing ----------------------------------------------------------------------------------------

// ---- cmu_wall -----------------------------------------------------------------------------------------------

// ---- concrete ---------------------------------------------------------------------------------------------------

// ---- rebar --------------------------------------------------------------------------------------------------------

// ---- anchor bolt ----------------------------------------------------------------------------------------------------

// ---- connector ------------------------------------------------------------------------------------------------------

// ---- truss -----------------------------------------------------------------------------------------------------------

// ---- membrane -----------------------------------------------------------------------------------------------------------

// ---- fill ------------------------------------------------------------------------------------------------------------------

// ---- insulation ----------------------------------------------------------------------------------------------------------------

// ---- solid ------------------------------------------------------------------------------------------------------------------------

// ---- flashing -----------------------------------------------------------------------------------------------------

// ---- joint --------------------------------------------------------------------------------------------------------

// ---- dispatch -----------------------------------------------------------------------------------------------------------------------

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
        inline for (@typeInfo(S).@"struct".fields) |f| {
            const Base = switch (@typeInfo(f.type)) {
                .optional => |o| o.child,
                else => f.type,
            };
            if (@typeInfo(Base) == .@"enum") {
                var found_row = false;
                for (rows) |r| {
                    if (!std.mem.eql(u8, r.names[0], f.name)) continue;
                    found_row = true;
                    inline for (@typeInfo(Base).@"enum".fields) |ef| {
                        if (std.mem.indexOf(u8, r.desc, ef.name) == null and std.mem.indexOf(u8, r.def, ef.name) == null) {
                            std.debug.print("{s}.{s}: choice '{s}' is not in its catalog text\n", .{ @typeName(S), f.name, ef.name });
                            return error.TestUnexpectedResult;
                        }
                    }
                }
                try std.testing.expect(found_row);
            }
        }
    }
}
