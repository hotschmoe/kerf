//! Kerf engine (Zig). Root of the importable module `kerf`.
//!
//! Typical use: `const r = try kerf.call(gpa, "export", input_json);` (`r.ok` false => `r.bytes` is
//! an error JSON). Everything allocates from the allocator you pass; the result is owned by you.

pub const api = @import("api.zig");
pub const json = @import("json.zig");
pub const units = @import("units.zig");
pub const num = @import("num.zig");
pub const limits = @import("limits.zig");
pub const oom = @import("oom.zig");
pub const geom = @import("geom.zig");
pub const clip = @import("clip.zig");
pub const font = @import("font.zig");
pub const style = @import("style.zig");
pub const model = @import("model.zig");
pub const catalog = @import("catalog.zig");
pub const scene = @import("scene.zig");
pub const pathgeom = @import("pathgeom.zig");
pub const builders = @import("builders.zig");
pub const compile = @import("compile.zig");
pub const testdocs = @import("testdocs.zig");
pub const hatch = @import("hatch.zig");
pub const pathclip = @import("pathclip.zig");
pub const drawing = @import("drawing.zig");
pub const view = @import("view.zig");
pub const section = @import("section.zig");
pub const drawview = @import("drawview.zig");
pub const textgeom = @import("textgeom.zig");
pub const svg = @import("svg.zig");
pub const canon = @import("canon.zig");
pub const validate = @import("validate.zig");
pub const annot = @import("annot.zig");
pub const route = @import("route.zig");
pub const layout_tests = @import("layout_tests.zig");
pub const views_tests = @import("views_tests.zig");
pub const v015_tests = @import("v015_tests.zig");
pub const hostile_tests = @import("hostile_tests.zig");
pub const iso = @import("iso.zig");
pub const mesh = @import("mesh.zig");
pub const sheet = @import("sheet.zig");
pub const dxf = @import("dxf.zig");
pub const pdf = @import("pdf.zig");
pub const png = @import("png.zig");
pub const raster = @import("raster.zig");
pub const tests = @import("tests.zig");
pub const load = @import("load.zig");
pub const ops = @import("ops.zig");

pub const Result = api.Result;
pub const call = api.call;
pub const version = api.version;

test {
    @import("std").testing.refAllDecls(@This());
}
