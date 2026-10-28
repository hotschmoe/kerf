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
use crate::geom::{self, Pt};
use crate::math;
use crate::pen::Pen;
use crate::png::{self, ColorType, EncodeError};
use crate::sort;
use crate::style::Style;
use crate::svg;
use crate::textgeom;
use alloc::vec;
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

/// Integer pixel kernels, over plain slices.
pub mod kernels {
    /// Composites `cov` (coverage 0..=255) onto `pix` (black ink) for the given row slices and clears `cov`.
    pub fn composite(pix: &mut [u8], cov: &mut [u8]) {
        for (p, c) in pix.iter_mut().zip(cov.iter_mut()) {
            let cv = u32::from(*c);
            if cv == 0 {
                continue;
            }
            *c = 0;
            let d = u32::from(*p);
            *p = ((d * (255 - cv) + 127) / 255) as u8;
        }
    }

    /// Union by max of a coverage value into the scratch.
    pub fn union_max(cov: &mut u8, v: u8) {
        if v > *cov {
            *cov = v;
        }
    }
}

#[derive(Clone, Copy)]
struct V {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

const SUB_SCANS: usize = 8;

struct Canvas {
    w: usize,
    h: usize,
    pix: Vec<u8>,
    cov: Vec<u8>,
    rmin: Vec<i32>,
    rmax: Vec<i32>,
    acc: Vec<f32>,
    flat: Vec<V>,
    edges: Vec<Edge>,
    xs: Vec<f64>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas {
            w,
            h,
            pix: vec![255; w * h],
            cov: vec![0; w * h],
            rmin: vec![i32::MAX; h],
            rmax: vec![-1; h],
            acc: vec![0.0; w + 2],
            flat: Vec::new(),
            edges: Vec::new(),
            xs: Vec::new(),
        }
    }

    fn mark(&mut self, y: usize, x0: usize, x1: usize) {
        self.rmin[y] = self.rmin[y].min(x0 as i32);
        self.rmax[y] = self.rmax[y].max(x1 as i32);
    }

    /// Composite the coverage scratch onto the page (black ink) and clear it.
    fn flush(&mut self) {
        for y in 0..self.h {
            if self.rmax[y] < 0 {
                continue;
            }
            let x0 = self.rmin[y] as usize;
            let x1 = self.rmax[y] as usize;
            let row = y * self.w;
            kernels::composite(&mut self.pix[row + x0..=row + x1], &mut self.cov[row + x0..=row + x1]);
            self.rmin[y] = i32::MAX;
            self.rmax[y] = -1;
        }
    }

    /// Union a round-capped segment of half-width `r` (pixels) into the coverage scratch.
    fn capsule(&mut self, ax: f64, ay: f64, bx: f64, by: f64, r: f64) {
        let rr = r + 0.5;
        let wf = self.w as f64;
        let hf = self.h as f64;
        let minx = ax.min(bx) - rr;
        let maxx = ax.max(bx) + rr;
        let miny = ay.min(by) - rr;
        let maxy = ay.max(by) + rr;
        if !(maxx > 0.0 && minx < wf && maxy > 0.0 && miny < hf) {
            return;
        }
        let ry0 = math::floor(miny - 0.5).max(0.0) as usize;
        let ry1 = math::ceil(maxy - 0.5).min(hf - 1.0) as usize;
        let dx = bx - ax;
        let dy = by - ay;
        let len2 = dx * dx + dy * dy;
        let len = math::sqrt(len2);
        let has_dir = len2 > 1e-12;
        let inv2 = if has_dir { 1.0 / len2 } else { 0.0 };
        let nx = if has_dir { -dy / len } else { 0.0 };
        let ny = if has_dir { dx / len } else { 1.0 };
        for y in ry0..=ry1 {
            let yc = y as f64 + 0.5;
            let mut xl = minx;
            let mut xr = maxx;
            if has_dir {
                let t = ny * (yc - ay);
                if nx.abs() < 1e-9 {
                    if t.abs() > rr {
                        continue;
                    }
                } else {
                    let s1 = ax + (-rr - t) / nx;
                    let s2 = ax + (rr - t) / nx;
                    xl = xl.max(s1.min(s2));
                    xr = xr.min(s1.max(s2));
                }
            }
            if xl > xr {
                continue;
            }
            let fx0 = math::floor(xl - 0.5).max(0.0);
            let fx1 = math::ceil(xr - 0.5).min(wf - 1.0);
            if fx0 > fx1 {
                continue;
            }
            let ix0 = fx0 as usize;
            let ix1 = fx1 as usize;
            let row = y * self.w;
            for x in ix0..=ix1 {
                let pxc = x as f64 + 0.5;
                let mut t = 0.0;
                if has_dir {
                    t = (((pxc - ax) * dx + (yc - ay) * dy) * inv2).clamp(0.0, 1.0);
                }
                let qx = ax + t * dx - pxc;
                let qy = ay + t * dy - yc;
                let cv = rr - math::sqrt(qx * qx + qy * qy);
                if cv <= 0.0 {
                    continue;
                }
                let v = math::round(cv.min(1.0) * 255.0) as u8;
                kernels::union_max(&mut self.cov[row + x], v);
            }
            self.mark(y, ix0, ix1);
        }
    }

    /// Stroke the flattened polyline in `self.flat` (pixel space), optionally dashed.
    fn stroke_flat(&mut self, r: f64, dash: Option<&[f64]>) {
        let p = core::mem::take(&mut self.flat);
        self.stroke_points(&p, r, dash);
        self.flat = p;
    }

    fn stroke_points(&mut self, p: &[V], r: f64, dash: Option<&[f64]>) {
        let Some(first) = p.first() else { return };
        if p.len() == 1 {
            self.capsule(first.x, first.y, first.x, first.y, r);
            return;
        }
        let pat_sum: f64 = dash.map_or(0.0, |d| d.iter().map(|v| v.max(0.0)).sum());
        let d = match dash {
            Some(d) if pat_sum >= 1e-6 && !d.is_empty() => d,
            _ => {
                for w in p.windows(2) {
                    self.capsule(w[0].x, w[0].y, w[1].x, w[1].y, r);
                }
                return;
            }
        };
        let n = if d.len() % 2 == 1 { d.len() * 2 } else { d.len() };
        let mut idx = 0usize;
        let mut rem = d[0].max(0.0);
        let mut on = true;
        for w in p.windows(2) {
            let (ax, ay) = (w[0].x, w[0].y);
            let sx = w[1].x - ax;
            let sy = w[1].y - ay;
            let len = math::sqrt(sx * sx + sy * sy);
            let mut pos = 0.0;
            while pos < len {
                let take = rem.min(len - pos);
                if on {
                    let t0 = pos / len;
                    let t1 = (pos + take) / len;
                    self.capsule(ax + sx * t0, ay + sy * t0, ax + sx * t1, ay + sy * t1, r);
                }
                pos += take;
                rem -= take;
                if rem <= 1e-9 {
                    idx = (idx + 1) % n;
                    rem = d[idx % d.len()].max(0.0);
                    on = !on;
                }
            }
        }
    }

    /// Even-odd fill of the edges in `self.edges` (pixel space) into the coverage scratch.
    fn fill_edges(&mut self) {
        if self.edges.is_empty() {
            return;
        }
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        for e in &self.edges {
            ymin = ymin.min(e.y0.min(e.y1));
            ymax = ymax.max(e.y0.max(e.y1));
        }
        let hf = self.h as f64;
        if !(ymax > 0.0 && ymin < hf) {
            return;
        }
        let wf = self.w as f64;
        let y0 = math::floor(ymin).max(0.0) as usize;
        let y1 = math::floor(ymax).min(hf - 1.0) as usize;
        let wgt = 1.0f32 / SUB_SCANS as f32;
        for y in y0..=y1 {
            let mut amin = self.w;
            let mut amax = 0usize;
            let mut any = false;
            for s in 0..SUB_SCANS {
                let sy = y as f64 + (s as f64 + 0.5) / SUB_SCANS as f64;
                self.xs.clear();
                for e in &self.edges {
                    if (e.y0 <= sy) != (e.y1 <= sy) {
                        self.xs.push(e.x0 + (sy - e.y0) * (e.x1 - e.x0) / (e.y1 - e.y0));
                    }
                }
                if self.xs.len() < 2 {
                    continue;
                }
                sort::stable(&mut self.xs, |a, b| a < b);
                for pair in self.xs.chunks_exact(2) {
                    let xa = pair[0].clamp(0.0, wf);
                    let xb = pair[1].clamp(0.0, wf);
                    if xb <= xa {
                        continue;
                    }
                    let ia = math::floor(xa) as usize;
                    let ib = math::floor(xb) as usize;
                    if !any {
                        self.acc.fill(0.0);
                        any = true;
                    }
                    amin = amin.min(ia);
                    amax = amax.max(ib);
                    if ia == ib {
                        self.acc[ia] += (xb - xa) as f32 * wgt;
                    } else {
                        self.acc[ia] += ((ia + 1) as f64 - xa) as f32 * wgt;
                        for x in ia + 1..ib {
                            self.acc[x] += wgt;
                        }
                        self.acc[ib] += (xb - ib as f64) as f32 * wgt;
                    }
                }
            }
            if !any {
                continue;
            }
            let row = y * self.w;
            let last = amax.min(self.w - 1);
            if amin > last {
                continue;
            }
            for x in amin..=last {
                let v = math::round(f64::from(self.acc[x].clamp(0.0, 1.0)) * 255.0) as u8;
                kernels::union_max(&mut self.cov[row + x], v);
            }
            self.mark(y, amin, last);
        }
    }
}

/// Flatten a bulge polyline (model units) into `out` in pixel space.
fn flatten(out: &mut Vec<V>, m: svg::Map, k: f64, pts: &[Pt], closed: bool) {
    let Some(first) = pts.first() else { return };
    out.push(V { x: m.px(first.x) * k, y: m.py(first.y) * k });
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
                out.push(V { x: m.px(pt.x) * k, y: m.py(pt.y) * k });
            }
        }
        out.push(V { x: m.px(q.x) * k, y: m.py(q.y) * k });
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
    let m = svg::Map { s: d.scale, x0: d.bounds[0], y1: d.bounds[3], ox: lay.margin_in, oy: lay.margin_in };
    let k = lay.ppi / svg::PX_PER_IN;
    let st = &*d.style;
    for it in &d.items {
        if matches!(it, Item::Region(_)) || !layer_known(d, it.layer()) {
            continue;
        }
        match it {
            Item::Region(_) => {}
            Item::Path(p) => {
                c.flat.clear();
                flatten(&mut c.flat, m, k, &p.pts, p.closed);
                let dash = pen_dash_px(st, p.pen, lay.ppi);
                c.stroke_flat(pen_width_px(st, p.pen, lay.ppi) * 0.5, dash.as_deref());
            }
            Item::Fill(f) => {
                c.edges.clear();
                for l in &f.loops {
                    c.flat.clear();
                    flatten(&mut c.flat, m, k, l, true);
                    if c.flat.len() < 2 {
                        continue;
                    }
                    let n = c.flat.len();
                    for i in 0..n {
                        let (a, b) = (c.flat[i], c.flat[(i + 1) % n]);
                        c.edges.push(Edge { x0: a.x, y0: a.y, x1: b.x, y1: b.y });
                    }
                }
                c.fill_edges();
            }
            Item::Hatch(h) => {
                let r = pen_width_px(st, h.pen, lay.ppi) * 0.5;
                let dash = pen_dash_px(st, h.pen, lay.ppi);
                for ln in &h.lines {
                    c.flat.clear();
                    c.flat.push(V { x: m.px(ln[0]) * k, y: m.py(ln[1]) * k });
                    if ln[0] != ln[2] || ln[1] != ln[3] {
                        c.flat.push(V { x: m.px(ln[2]) * k, y: m.py(ln[3]) * k });
                    }
                    c.stroke_flat(r, dash.as_deref());
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
                    c.flat.clear();
                    c.flat.extend(pl.iter().map(|p| V { x: m.px(p.x) * k, y: m.py(p.y) * k }));
                    c.stroke_flat(r, dash.as_deref());
                }
            }
        }
        c.flush();
    }
    Image { width: lay.w, height: lay.h, pixels: c.pix }
}

/// PNG bytes (gray8) of the drawing.
pub fn render(d: &Drawing<'_>, font: &Font, opt: &Options) -> Result<Vec<u8>, EncodeError> {
    let img = rasterize(d, font, opt);
    png::encode(img.width, img.height, ColorType::Gray, &img.pixels)
}
