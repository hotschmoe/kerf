//! Kerf draw layer (pure logic, std only, wasm32-freestanding + native).
//!
//! Pipeline: `ir.parse` (Drawing JSON) -> `tess.Tessellator.build` (screen-space triangles, shared by the
//! live viewport and the PNG renderer) -> `raster.Image.drawTris` -> `png.encode`.
//!
//! | module   | what |
//! |----------|------|
//! | `ir`     | Drawing IR types + tolerant parser (`ir.parse`), `srcBase` / `srcMatches` for `id#k` instance srcs |
//! | `mesh`   | Mesh IR (`kerf_mesh`) types + parser |
//! | `font`   | Kerf stroke font: `Font.initEmbedded`, `textStrokes`, `textWidth` |
//! | `geom`   | bulge arcs, flattening, point-in-polygon with arcs, ear-clipping triangulation |
//! | `tess`   | `Tessellator`, `View`, `Palette`, `Options`, `TriBuf`/`Vert` (the one shared tessellator) |
//! | `pick`   | `pick`, `pickHit`, `bboxOfSrc`, `textOrigin` (hover / select / note drag) |
//! | `raster` | CPU triangle rasterizer (`Image`) |
//! | `png`    | PNG encoder (gray / RGB / RGBA, filters + deflate) |
//! | `render` | `renderPng` (kerf_render: white bg, black ink, ~1400 px), `renderLivePng`, `renderView` |
//!
//! `render_cli.zig` (native dev tool, reads/writes files) is deliberately NOT imported here.
//! Needs the anonymous import `spec_font_json` (see apps/teak/build.zig) for `Font.initEmbedded`.

pub const geom = @import("geom.zig");
pub const ir = @import("ir.zig");
pub const mesh = @import("mesh.zig");
pub const font = @import("font.zig");
pub const tess = @import("tess.zig");
pub const pick = @import("pick.zig");
pub const raster = @import("raster.zig");
pub const png = @import("png.zig");
pub const render = @import("render.zig");

pub const Drawing = ir.Drawing;
pub const Mesh = mesh.Mesh;
pub const Font = font.Font;
pub const Tessellator = tess.Tessellator;
pub const View = tess.View;
pub const Palette = tess.Palette;
pub const renderPng = render.renderPng;

test {
    _ = @import("geom.zig");
    _ = @import("ir.zig");
    _ = @import("font.zig");
    _ = @import("mesh.zig");
    _ = @import("tess.zig");
    _ = @import("raster.zig");
    _ = @import("png.zig");
    _ = @import("render.zig");
    _ = @import("pick.zig");
    _ = @import("fixtures_test.zig");
}
