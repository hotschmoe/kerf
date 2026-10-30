//! The raster canvas: a gray8 page (255 = paper), a per-item coverage scratch and per-row dirty spans.
//!
//! Every float is converted to an index with `as`, which saturates (NaN gives 0), after the surrounding float logic has
//! clamped it to the buffer, so no input can index outside a buffer. Float operations are written in a fixed order: the
//! pixels are part of the golden files.

use crate::geom::V2;
use crate::math::{ceil, floor, round, sqrt};
use alloc::vec::Vec;

/// Longest image side.
pub const MAX_SIDE: usize = 8192;

/// Weight of one of the 8 sub-scanlines of a fill, in `f32` like the accumulator.
const SUB_WEIGHT: f32 = 1.0 / 8.0;
const SUB_SCANLINES: usize = 8;

#[derive(Clone, Copy)]
pub struct Edge {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

pub struct Canvas {
    w: usize,
    h: usize,
    pix: Vec<u8>,
    cov: Vec<u8>,
    /// Per row, the columns `(first, last)` the coverage scratch has touched.
    dirty: Vec<Option<(usize, usize)>>,
    /// Per-column coverage of one fill row (`w + 2` entries: crossings may land on `w`).
    acc: Vec<f32>,
    /// Edge crossings of one sub-scanline.
    xs: Vec<f64>,
}

impl Canvas {
    /// A white page of `w` x `h` pixels, each clamped to `1..=MAX_SIDE` (`raster::layout` already keeps them there).
    pub fn new(w: usize, h: usize) -> Canvas {
        let (w, h) = (w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE));
        Canvas {
            w,
            h,
            pix: alloc::vec![255; w * h],
            cov: alloc::vec![0; w * h],
            dirty: alloc::vec![None; h],
            acc: alloc::vec![0.0; w + 2],
            xs: Vec::new(),
        }
    }

    pub fn into_pixels(self) -> Vec<u8> {
        self.pix
    }

    fn mark(&mut self, y: usize, first: usize, last: usize) {
        let span = self.dirty[y].get_or_insert((first, last));
        span.0 = span.0.min(first);
        span.1 = span.1.max(last);
    }

    /// Raises the coverage of `(x, y)` to `v`.
    fn raise(&mut self, y: usize, x: usize, v: u8) {
        let c = &mut self.cov[y * self.w + x];
        *c = (*c).max(v);
    }

    /// Composites the coverage scratch onto the page (`dst *= 1 - coverage`) and clears it.
    pub fn flush(&mut self) {
        for (y, span) in self.dirty.iter_mut().enumerate() {
            let Some((first, last)) = span.take() else { continue };
            let row = y * self.w;
            let cov = &mut self.cov[row + first..=row + last];
            for (cov, pix) in cov.iter_mut().zip(&mut self.pix[row + first..=row + last]) {
                if *cov != 0 {
                    let (c, d) = (u32::from(*cov), u32::from(*pix));
                    *pix = ((d * (255 - c) + 127) / 255) as u8;
                    *cov = 0;
                }
            }
        }
    }

    /// Unions a round-capped segment of half-width `r` (pixels) into the coverage scratch:
    /// coverage = clamp(r + 0.5 - distance, 0, 1) per pixel, rows spanned analytically.
    pub fn capsule(&mut self, a: V2, b: V2, r: f64) {
        let (wf, hf) = (self.w as f64, self.h as f64);
        let rr = r + 0.5;
        let (minx, maxx) = (a.x.min(b.x) - rr, a.x.max(b.x) + rr);
        let (miny, maxy) = (a.y.min(b.y) - rr, a.y.max(b.y) + rr);
        if !(0.0 < maxx && minx < wf && 0.0 < maxy && miny < hf) {
            return;
        }
        let ry0 = 0.0f64.max(floor(miny - 0.5)) as usize;
        let ry1 = ((hf - 1.0).min(ceil(maxy - 0.5)) as usize).min(self.h - 1);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let len = sqrt(len2);
        let has_dir = 1e-12 < len2;
        let inv2 = if has_dir { 1.0 / len2 } else { 0.0 };
        let (nx, ny) = if has_dir { (-dy / len, dx / len) } else { (0.0, 1.0) };
        for y in ry0..=ry1 {
            let yc = y as f64 + 0.5;
            let (mut xl, mut xr) = (minx, maxx);
            if has_dir {
                let t = ny * (yc - a.y);
                if nx.abs() < 1e-9 {
                    if rr < t.abs() {
                        continue;
                    }
                } else {
                    let s1 = a.x + (-rr - t) / nx;
                    let s2 = a.x + (rr - t) / nx;
                    xl = xl.max(s1.min(s2));
                    xr = xr.min(s1.max(s2));
                }
            }
            if xr < xl {
                continue;
            }
            let fx0 = 0.0f64.max(floor(xl - 0.5));
            let fx1 = (wf - 1.0).min(ceil(xr - 0.5));
            if fx1 < fx0 {
                continue;
            }
            let (ix0, ix1) = (fx0 as usize, (fx1 as usize).min(self.w - 1));
            if ix0 > ix1 {
                continue;
            }
            for x in ix0..=ix1 {
                let pxc = x as f64 + 0.5;
                let mut t = 0.0;
                if has_dir {
                    t = ((pxc - a.x) * dx + (yc - a.y) * dy) * inv2;
                    t = 0.0f64.max(t.min(1.0));
                }
                let qx = a.x + t * dx - pxc;
                let qy = a.y + t * dy - yc;
                let cv = rr - sqrt(qx * qx + qy * qy);
                if cv > 0.0 {
                    self.raise(y, x, round(cv.min(1.0) * 255.0) as u8);
                }
            }
            self.mark(y, ix0, ix1);
        }
    }

    /// Strokes a flattened polyline (pixel space), optionally dashed (an odd dash list repeats,
    /// as in SVG). The dash walker advances by float amounts and ends because the style admits
    /// only dashes of at least 0.01 mm.
    pub fn stroke(&mut self, p: &[V2], r: f64, dash: Option<&[f64]>) {
        match p {
            [] => {}
            [only] => self.capsule(*only, *only, r),
            _ => match dash.filter(|d| !d.is_empty() && d.iter().fold(0.0, |s, x| s + x.max(0.0)) >= 1e-6) {
                Some(dash) => self.stroke_dashed(p, r, dash),
                None => p.windows(2).for_each(|seg| self.capsule(seg[0], seg[1], r)),
            },
        }
    }

    fn stroke_dashed(&mut self, p: &[V2], r: f64, dash: &[f64]) {
        let n = if dash.len() % 2 == 1 { dash.len() * 2 } else { dash.len() };
        let mut idx = 0;
        let mut rem = dash[0].max(0.0);
        let mut on = true;
        for seg in p.windows(2) {
            let (a, s) = (seg[0], V2 { x: seg[1].x - seg[0].x, y: seg[1].y - seg[0].y });
            let len = sqrt(s.x * s.x + s.y * s.y);
            let mut pos = 0.0;
            while pos < len {
                let take = rem.min(len - pos);
                if on {
                    let (t0, t1) = (pos / len, (pos + take) / len);
                    self.capsule(
                        V2 { x: a.x + s.x * t0, y: a.y + s.y * t0 },
                        V2 { x: a.x + s.x * t1, y: a.y + s.y * t1 },
                        r,
                    );
                }
                pos += take;
                rem -= take;
                if rem <= 1e-9 {
                    idx = (idx + 1) % n;
                    rem = dash[idx % dash.len()].max(0.0);
                    on = !on;
                }
            }
        }
    }

    /// Even-odd fill of the closed edges (pixel space) into the coverage scratch: 8 sub-scanlines
    /// per row with exact horizontal coverage, crossings sorted with the stable sort.
    pub fn fill(&mut self, es: &[Edge]) {
        let (wf, hf) = (self.w as f64, self.h as f64);
        let (ymin, ymax) = es
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), e| (lo.min(e.y0.min(e.y1)), hi.max(e.y0.max(e.y1))));
        if es.is_empty() || !(0.0 < ymax && ymin < hf) {
            return;
        }
        let y0 = 0.0f64.max(floor(ymin)) as usize;
        let y1 = ((hf - 1.0).min(floor(ymax)) as usize).min(self.h - 1);
        for y in y0..=y1 {
            let mut touched: Option<(usize, usize)> = None;
            for s in 0..SUB_SCANLINES {
                let sy = y as f64 + (s as f64 + 0.5) / 8.0;
                self.xs.clear();
                self.xs.extend(
                    es.iter()
                        .filter(|e| (e.y0 <= sy) != (e.y1 <= sy))
                        .map(|e| e.x0 + (sy - e.y0) * (e.x1 - e.x0) / (e.y1 - e.y0)),
                );
                crate::sort::stable(&mut self.xs, |a, b| a < b);
                for &[x0, x1] in self.xs.as_chunks::<2>().0 {
                    let xa = 0.0f64.max(x0.min(wf));
                    let xb = 0.0f64.max(x1.min(wf));
                    if xb <= xa {
                        continue;
                    }
                    let (ia, ib) = ((floor(xa) as usize).min(self.w), (floor(xb) as usize).min(self.w));
                    if touched.is_none() {
                        self.acc.fill(0.0);
                    }
                    let (lo, hi) = touched.get_or_insert((ia, ib));
                    *lo = (*lo).min(ia);
                    *hi = (*hi).max(ib);
                    let acc = &mut self.acc;
                    let mut add = |i: usize, x: f64| acc[i] += x as f32 * SUB_WEIGHT;
                    if ia == ib {
                        add(ia, xb - xa);
                    } else {
                        add(ia, (ia + 1) as f64 - xa);
                        (ia + 1..ib).for_each(|x| add(x, 1.0));
                        add(ib, xb - ib as f64);
                    }
                }
            }
            let Some((first, last)) = touched else { continue };
            let last = last.min(self.w - 1);
            if first <= last {
                for x in first..=last {
                    // min/max rather than clamp: a NaN accumulator must give 0 coverage, not propagate
                    #[allow(clippy::manual_clamp)]
                    let v = round(f64::from(self.acc[x].min(1.0).max(0.0) * 255.0)) as u8;
                    self.raise(y, x, v);
                }
                self.mark(y, first, last);
            }
        }
    }
}
