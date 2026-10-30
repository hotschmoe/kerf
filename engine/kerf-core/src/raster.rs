//! PNG raster exporter (SPEC 12): renders exactly the items the SVG exporter shows (same Map, same pens,
//! dashes, round caps/joins, even-odd fills, hatch lines, stroke-font text) into an 8-bit grayscale image
//! (white paper, black ink) and encodes it with `png`.
//!
//! Rasterizer: every item is drawn into a coverage scratch (u8, union by max, so overlapping segments of
//! one stroke never double-darken), then composited once onto the page (`dst *= 1 - coverage`).
//!  * strokes: each flattened segment is a capsule; coverage = clamp(r + 0.5 - distance, 0, 1) per pixel
//!    (a 1 px line is exactly one pixel wide). Row spans are computed analytically, so a long diagonal
//!    hatch line costs O(length), not O(bbox).
//!  * fills: even-odd scanline fill, 8 sub-scanlines per pixel row with exact horizontal coverage.

use crate::drawing::{Drawing, Item};
use crate::font::Font;
use crate::geom::{self, Pt, V2};
use crate::math;
use crate::pen::Pen;
use crate::png::{self, EncodeError};
use crate::style::Style;
use crate::svg;
use crate::textgeom;
use alloc::vec::Vec;

pub const DEFAULT_PX: u32 = 1600;
pub const MIN_PX: u32 = 200;
pub const MAX_PX: u32 = 6000;
/// Longest side the page may reach (a tall sheet at 6000 px wide would otherwise be enormous).
pub const MAX_SIDE: u32 = 8192;

pub struct Options {
    /// Requested image width in pixels (clamped to MIN_PX..=MAX_PX).
    pub px: u32,
    /// Paper margin around a bare (non-sheet) detail, as in the SVG exporter.
    pub margin_in: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { px: DEFAULT_PX, margin_in: 0.25 }
    }
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Gray8, row 0 = top, 255 = white paper.
    pub pixels: Vec<u8>,
}

/// The canvas: coverage scratch, per-row dirty spans, strokes and fills.
mod canvas;

use canvas::{Canvas, Edge};

/// Flatten a bulge polyline (model units) into `out` in pixel space.
fn flatten(out: &mut Vec<V2>, m: svg::Map, k: f64, pts: &[Pt], closed: bool) {
    let Some(first) = pts.first() else { return };
    out.push(V2 { x: m.px(first.x) * k, y: m.py(first.y) * k });
    let nseg = if closed { pts.len() } else { pts.len() - 1 };
    for i in 0..nseg {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        if p.b != 0.0 {
            let arc = geom::arc_of(p.v(), q.v(), p.b);
            let r_px = arc.r / m.s * svg::PX_PER_IN * k;
            let step = if r_px > 0.04 { 2.0 * math::acos(1.0 - 0.04 / r_px) } else { math::PI };
            let nf = math::ceil(arc.sweep.abs() / step.max(1e-3));
            let segs = nf.clamp(2.0, 4096.0) as usize;
            for j in 1..segs {
                let pt = arc.at(j as f64 / segs as f64);
                out.push(V2 { x: m.px(pt.x) * k, y: m.py(pt.y) * k });
            }
        }
        out.push(V2 { x: m.px(q.x) * k, y: m.py(q.y) * k });
    }
}

fn pen_width_px(st: &Style, pen: Pen, ppi: f64) -> f64 {
    let w_mm = st.pen(pen).map_or(0.25, |p| p.width_mm);
    (w_mm / 25.4 * ppi).max(1.0)
}

fn pen_dash_px(st: &Style, pen: Pen, ppi: f64) -> Option<Vec<f64>> {
    let dm = st.pen(pen)?.dash_mm.as_ref()?;
    Some(dm.iter().map(|v| v / 25.4 * ppi).collect())
}

fn layer_known(d: &Drawing<'_>, name: &str) -> bool {
    d.layers.iter().any(|l| l.name == name)
}

/// Page size in pixels and pixels per paper inch for the drawing at the requested width.
pub struct Layout {
    pub w: u32,
    pub h: u32,
    pub ppi: f64,
    pub margin_in: f64,
}

pub fn layout(d: &Drawing<'_>, opt: &Options) -> Layout {
    let s = d.scale;
    let margin = if d.page_w > 0.0 { 0.0 } else { opt.margin_in };
    let w_in = (d.bounds[2] - d.bounds[0]) / s + 2.0 * margin;
    let h_in = (d.bounds[3] - d.bounds[1]) / s + 2.0 * margin;
    let want = f64::from(opt.px.clamp(MIN_PX, MAX_PX));
    let mut ppi = want / w_in.max(1e-6);
    ppi = ppi.min(f64::from(MAX_SIDE) / h_in.max(1e-6));
    let side = |v: f64| math::ceil(v * ppi - 1e-9).clamp(1.0, f64::from(MAX_SIDE)) as u32;
    Layout { w: side(w_in), h: side(h_in), ppi, margin_in: margin }
}

pub fn rasterize(d: &Drawing<'_>, font: &Font, opt: &Options) -> Image {
    let lay = layout(d, opt);
    let mut c = Canvas::new(lay.w as usize, lay.h as usize);
    let mut flat: Vec<V2> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let m = svg::Map { s: d.scale, x0: d.bounds[0], y1: d.bounds[3], ox: lay.margin_in, oy: lay.margin_in };
    let k = lay.ppi / svg::PX_PER_IN;
    let st = d.style;
    for it in &d.items {
        if matches!(it, Item::Region(_)) || !layer_known(d, it.layer()) {
            continue;
        }
        match it {
            Item::Region(_) => {}
            Item::Path(p) => {
                flat.clear();
                flatten(&mut flat, m, k, &p.pts, p.closed);
                let dash = pen_dash_px(st, p.pen, lay.ppi);
                c.stroke(&flat, pen_width_px(st, p.pen, lay.ppi) * 0.5, dash.as_deref());
            }
            Item::Fill(f) => {
                edges.clear();
                for l in &f.loops {
                    flat.clear();
                    flatten(&mut flat, m, k, l, true);
                    if flat.len() < 2 {
                        continue;
                    }
                    let n = flat.len();
                    for i in 0..n {
                        let (a, b) = (flat[i], flat[(i + 1) % n]);
                        edges.push(Edge { x0: a.x, y0: a.y, x1: b.x, y1: b.y });
                    }
                }
                c.fill(&edges);
            }
            Item::Hatch(h) => {
                let r = pen_width_px(st, h.pen, lay.ppi) * 0.5;
                let dash = pen_dash_px(st, h.pen, lay.ppi);
                for ln in &h.lines {
                    flat.clear();
                    flat.push(V2 { x: m.px(ln[0]) * k, y: m.py(ln[1]) * k });
                    if ln[0] != ln[2] || ln[1] != ln[3] {
                        flat.push(V2 { x: m.px(ln[2]) * k, y: m.py(ln[3]) * k });
                    }
                    c.stroke(&flat, r, dash.as_deref());
                }
            }
            Item::Text(t) => {
                let sts = textgeom::strokes(font, t);
                let r = pen_width_px(st, t.pen, lay.ppi) * 0.5;
                let dash = pen_dash_px(st, t.pen, lay.ppi);
                for pl in &sts {
                    if pl.len() < 2 {
                        continue;
                    }
                    flat.clear();
                    flat.extend(pl.iter().map(|p| V2 { x: m.px(p.x) * k, y: m.py(p.y) * k }));
                    c.stroke(&flat, r, dash.as_deref());
                }
            }
        }
        c.flush();
    }
    Image { width: lay.w, height: lay.h, pixels: c.into_pixels() }
}

/// PNG bytes (gray8) of the drawing.
pub fn render(d: &Drawing<'_>, font: &Font, opt: &Options) -> Result<Vec<u8>, EncodeError> {
    let img = rasterize(d, font, opt);
    png::encode_gray(img.width, img.height, &img.pixels)
}
