//! Iso view (placeholder until the HLR implementation lands).
const geom = @import("geom.zig");
const scene_mod = @import("scene.zig");

pub const Iso = struct {
    pub fn landing(self: *Iso, comp: *const scene_mod.Comp, inst: ?u32, part: ?[]const u8) ?geom.V2 {
        _ = self;
        _ = comp;
        _ = inst;
        _ = part;
        return null;
    }
    pub fn project(self: *Iso, p: geom.V2) ?geom.V2 {
        _ = self;
        _ = p;
        return null;
    }
};
