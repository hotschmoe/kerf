//! The raster canvas (raster.zig's `Canvas`), proven with Verus (engines/rust/verus/VERUS.md): no write outside the
//! pixel, coverage, accumulator or span buffers for any float input, integer arithmetic overflow-free.

use alloc::vec::Vec;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

/// Longest image side (raster.zig `max_side`).
#[cfg_attr(verus_keep_ghost, verus_verify)]
pub const MAX_SIDE: usize = 8192;

/// The float functions core lacks on stable `no_std`, from the engine's `math` (exact, identical on every target).
/// Trusted: nothing about their results is assumed; every index written is proven in bounds whatever they return.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[inline(always)]
fn floor(x: f64) -> f64 {
    crate::math::floor(x)
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[inline(always)]
fn ceil(x: f64) -> f64 {
    crate::math::ceil(x)
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[inline(always)]
fn sqrt(x: f64) -> f64 {
    crate::math::sqrt(x)
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[inline(always)]
fn round(x: f64) -> f64 {
    crate::math::round(x)
}

#[derive(Clone, Copy)]
#[cfg_attr(verus_keep_ghost, verus_verify)]
pub struct Edge {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

#[derive(Clone, Copy)]
#[cfg_attr(verus_keep_ghost, verus_verify)]
pub struct V {
    pub x: f64,
    pub y: f64,
}

// ---- trusted float boundary: total Rust operations, no properties assumed ------------------------

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn fmin(a: f64, b: f64) -> f64 {
    a.min(b)
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn fmax(a: f64, b: f64) -> f64 {
    a.max(b)
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn add(a: f64, b: f64) -> f64 {
    a + b
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn sub(a: f64, b: f64) -> f64 {
    a - b
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn mul(a: f64, b: f64) -> f64 {
    a * b
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn div(a: f64, b: f64) -> f64 {
    a / b
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn lt(a: f64, b: f64) -> bool {
    a < b
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn le(a: f64, b: f64) -> bool {
    a <= b
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn abs(a: f64) -> f64 {
    a.abs()
}

/// `@intFromFloat` of a value the float logic has already clamped to `[0, n]`; saturating (NaN
/// gives 0), so it is total for any input.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn to_index(x: f64) -> usize {
    x as usize
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn to_u8(x: f64) -> u8 {
    x as u8
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
#[inline(always)]
fn from_usize(n: usize) -> f64 {
    n as f64
}

/// `acc[i] += (x as f32) * wgt` with raster.zig's f32 accumulator.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        i < old(acc)@.len(),
    ensures
        final(acc)@.len() == old(acc)@.len(),
))]
#[inline(always)]
fn acc_add(acc: &mut Vec<f32>, i: usize, x: f64) {
    acc[i] += x as f32 * (1.0f32 / 8.0);
}

/// `@round(clamp(acc[i], 0, 1) * 255)` as a byte (f32 arithmetic, as raster.zig).
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        i < acc@.len(),
))]
#[inline(always)]
fn acc_byte(acc: &Vec<f32>, i: usize) -> u8 {
    round((acc[i].min(1.0).max(0.0) * 255.0) as f64) as u8
}

// ---- canvas ---------------------------------------------------------------------------------------

/// raster.zig's `Canvas`: gray8 page (255 = paper), per-item coverage scratch and per-row dirty spans.
#[cfg_attr(verus_keep_ghost, verus_verify)]
pub struct Canvas {
    w: usize,
    h: usize,
    pix: Vec<u8>,
    cov: Vec<u8>,
    rmin: Vec<i32>,
    rmax: Vec<i32>,
    acc: Vec<f32>,
    xs: Vec<f64>,
}

#[cfg(verus_keep_ghost)]
verus! {

impl Canvas {
/// Buffer sizes agree with the dimensions and every dirty span lies inside its row.
    pub closed spec fn wf(&self) -> bool {
        &&& 1 <= self.w <= MAX_SIDE
        &&& 1 <= self.h <= MAX_SIDE
        &&& self.pix@.len() == self.w * self.h
        &&& self.cov@.len() == self.w * self.h
        &&& self.rmin@.len() == self.h
        &&& self.rmax@.len() == self.h
        &&& self.acc@.len() == self.w + 2
        &&& forall|y: int|
            0 <= y < self.h ==> (#[trigger] self.rmax@[y] < 0 && self.rmin@[y] == i32::MAX) || (0
                <= self.rmin@[y] <= self.rmax@[y] && self.rmax@[y] < self.w)
    }

pub closed spec fn width(&self) -> nat {
        self.w as nat
    }

pub closed spec fn height(&self) -> nat {
        self.h as nat
    }
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_verify)]
impl Canvas {
    /// A white page of `w` x `h` pixels, each clamped to `1..=MAX_SIDE` (`raster::layout` already keeps them there).
    #[cfg_attr(verus_keep_ghost, verus_spec(c =>
        ensures
            c.wf(),
            c.width() == if w < 1 { 1 } else if w > MAX_SIDE { MAX_SIDE } else { w },
            c.height() == if h < 1 { 1 } else if h > MAX_SIDE { MAX_SIDE } else { h },
    ))]
    pub fn new(w: usize, h: usize) -> Canvas {
        let w = if w < 1 { 1 } else if w > MAX_SIDE { MAX_SIDE } else { w };
        let h = if h < 1 { 1 } else if h > MAX_SIDE { MAX_SIDE } else { h };
        #[cfg(verus_keep_ghost)]
        proof! { assert(w * h <= MAX_SIDE * MAX_SIDE) by (nonlinear_arith)
            requires
                w <= MAX_SIDE,
                h <= MAX_SIDE,
        ; }
        let n = w * h;
        let pix = fill_u8(255, n);
        let cov = fill_u8(0, n);
        let mut rmin: Vec<i32> = Vec::with_capacity(h);
        let mut rmax: Vec<i32> = Vec::with_capacity(h);
        let mut y: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                y <= h,
                rmin@.len() == y,
                rmax@.len() == y,
                forall|k: int| 0 <= k < y ==> #[trigger] rmax@[k] == -1 && rmin@[k] == i32::MAX,
            decreases h - y,
        ))]
        while y < h {
            rmin.push(i32::MAX);
            rmax.push(-1);
            y += 1;
        }
        let mut acc: Vec<f32> = Vec::with_capacity(w + 2);
        let mut x: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                w <= MAX_SIDE,
                x <= w + 2,
                acc@.len() == x,
            decreases w + 2 - x,
        ))]
        while x < w + 2 {
            acc.push(0.0);
            x += 1;
        }
        Canvas { w, h, pix, cov, rmin, rmax, acc, xs: Vec::new() }
    }

    #[cfg_attr(verus_keep_ghost, verus_spec)]
    pub fn width_px(&self) -> usize {
        self.w
    }

    #[cfg_attr(verus_keep_ghost, verus_spec)]
    pub fn height_px(&self) -> usize {
        self.h
    }

    /// The page, row 0 at the top.
    #[cfg_attr(verus_keep_ghost, verus_spec)]
    pub fn pixels(&self) -> &[u8] {
        self.pix.as_slice()
    }

    #[cfg_attr(verus_keep_ghost, verus_spec)]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pix
    }

    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
            y < old(self).h,
            x0 <= x1 < old(self).w,
        ensures
            final(self).wf(),
            final(self).w == old(self).w,
            final(self).h == old(self).h,
    ))]
    fn mark(&mut self, y: usize, x0: usize, x1: usize) {
        if (x0 as i32) < self.rmin[y] {
            self.rmin[y] = x0 as i32;
        }
        if (x1 as i32) > self.rmax[y] {
            self.rmax[y] = x1 as i32;
        }
    }

    /// Composites the coverage scratch onto the page (`dst *= 1 - coverage`) and clears it.
    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    ))]
    pub fn flush(&mut self) {
        let w = self.w;
        let h = self.h;
        let mut y: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                self.wf(),
                w == self.w,
                h == self.h,
                y <= h,
            decreases h - y,
        ))]
        while y < h {
            if self.rmax[y] >= 0 {
                let x0 = self.rmin[y] as usize;
                let x1 = self.rmax[y] as usize;
                #[cfg(verus_keep_ghost)]
                proof! {
                    lemma_row(y, w, h);
                }
                let row = y * w;
                let mut x = x0;
                #[cfg_attr(verus_keep_ghost, verus_spec(
                    invariant
                        self.wf(),
                        w == self.w,
                        h == self.h,
                        y < h,
                        row + w <= w * h <= MAX_SIDE * MAX_SIDE,
                        x0 <= x,
                        x1 < w,
                    decreases x1 + 1 - x,
                ))]
                while x <= x1 {
                    let cv = self.cov[row + x] as u32;
                    if cv != 0 {
                        self.cov[row + x] = 0;
                        let d = self.pix[row + x] as u32;
                        #[cfg(verus_keep_ghost)]
                        proof! { assert(d * (255 - cv) + 127 <= 255 * 255 + 127) by (nonlinear_arith)
                            requires
                                d <= 255,
                                cv <= 255,
                        ; }
                        #[cfg(verus_keep_ghost)]
                        proof! { assert((d * (255 - cv) + 127) / 255 <= 255) by (nonlinear_arith)
                            requires
                                d <= 255,
                                cv <= 255,
                        ; }
                        self.pix[row + x] = ((d * (255 - cv) + 127) / 255) as u8;
                    }
                    x += 1;
                }
                self.rmin[y] = i32::MAX;
                self.rmax[y] = -1;
            }
            y += 1;
        }
    }

    /// Unions a round-capped segment of half-width `r` (pixels) into the coverage scratch:
    /// coverage = clamp(r + 0.5 - distance, 0, 1) per pixel, rows spanned analytically.
    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    ))]
    pub fn capsule(&mut self, ax: f64, ay: f64, bx: f64, by: f64, r: f64) {
        let w = self.w;
        let h = self.h;
        let rr = add(r, 0.5);
        let wf = from_usize(w);
        let hf = from_usize(h);
        let minx = sub(fmin(ax, bx), rr);
        let maxx = add(fmax(ax, bx), rr);
        let miny = sub(fmin(ay, by), rr);
        let maxy = add(fmax(ay, by), rr);
        if !(lt(0.0, maxx) && lt(minx, wf) && lt(0.0, maxy) && lt(miny, hf)) {
            return;
        }
        let ry0 = to_index(fmax(0.0, floor(sub(miny, 0.5))));
        let ry1 = min(to_index(fmin(sub(hf, 1.0), ceil(sub(maxy, 0.5)))), h - 1);
        let dx = sub(bx, ax);
        let dy = sub(by, ay);
        let len2 = add(mul(dx, dx), mul(dy, dy));
        let len = sqrt(len2);
        let has_dir = lt(1e-12, len2);
        let inv2 = if has_dir { div(1.0, len2) } else { 0.0 };
        let nx = if has_dir { div(sub(0.0, dy), len) } else { 0.0 };
        let ny = if has_dir { div(dx, len) } else { 1.0 };
        let mut y = ry0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                self.wf(),
                w == self.w,
                h == self.h,
                ry1 < h,
            decreases ry1 + 1 - y,
        ))]
        while y <= ry1 {
            let yc = add(from_usize(y), 0.5);
            let mut xl = minx;
            let mut xr = maxx;
            let mut skip = false;
            if has_dir {
                let t = mul(ny, sub(yc, ay));
                if lt(abs(nx), 1e-9) {
                    if lt(rr, abs(t)) {
                        skip = true;
                    }
                } else {
                    let s1 = add(ax, div(sub(sub(0.0, rr), t), nx));
                    let s2 = add(ax, div(sub(rr, t), nx));
                    xl = fmax(xl, fmin(s1, s2));
                    xr = fmin(xr, fmax(s1, s2));
                }
            }
            if !skip && !lt(xr, xl) {
                let fx0 = fmax(0.0, floor(sub(xl, 0.5)));
                let fx1 = fmin(sub(wf, 1.0), ceil(sub(xr, 0.5)));
                if !lt(fx1, fx0) {
                    let ix0 = to_index(fx0);
                    let ix1 = min(to_index(fx1), w - 1);
                    if ix0 <= ix1 {
                        #[cfg(verus_keep_ghost)]
                        proof! {
                            lemma_row(y, w, h);
                        }
                        let row = y * w;
                        let mut x = ix0;
                        #[cfg_attr(verus_keep_ghost, verus_spec(
                            invariant
                                self.wf(),
                                w == self.w,
                                h == self.h,
                                y < h,
                                row + w <= w * h <= MAX_SIDE * MAX_SIDE,
                                ix1 < w,
                            decreases ix1 + 1 - x,
                        ))]
                        while x <= ix1 {
                            let pxc = add(from_usize(x), 0.5);
                            let mut t = 0.0;
                            if has_dir {
                                t = mul(add(mul(sub(pxc, ax), dx), mul(sub(yc, ay), dy)), inv2);
                                t = fmax(0.0, fmin(t, 1.0));
                            }
                            let qx = sub(add(ax, mul(t, dx)), pxc);
                            let qy = sub(add(ay, mul(t, dy)), yc);
                            let cv = sub(rr, sqrt(add(mul(qx, qx), mul(qy, qy))));
                            if !le(cv, 0.0) {
                                let v = to_u8(round(mul(fmin(cv, 1.0), 255.0)));
                                if v > self.cov[row + x] {
                                    self.cov[row + x] = v;
                                }
                            }
                            x += 1;
                        }
                        self.mark(y, ix0, ix1);
                    }
                }
            }
            y += 1;
        }
    }

    /// Strokes a flattened polyline (pixel space), optionally dashed (an odd dash list repeats,
    /// as in SVG). Termination is not proven: the dash walker advances by float amounts (it ends
    /// because the style admits only dashes of at least 0.01 mm).
    #[cfg_attr(verus_keep_ghost, verifier::exec_allows_no_decreases_clause)]
    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    ))]
    pub fn stroke(&mut self, p: &[V], r: f64, dash: Option<&[f64]>) {
        if p.len() == 0 {
            return;
        }
        if p.len() == 1 {
            self.capsule(p[0].x, p[0].y, p[0].x, p[0].y, r);
            return;
        }
        let mut pat_sum = 0.0;
        let d: &[f64] = match dash {
            Some(d) => d,
            None => &[],
        };
        let mut k: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                k <= d@.len(),
            decreases d.len() - k,
        ))]
        while k < d.len() {
            pat_sum = add(pat_sum, fmax(d[k], 0.0));
            k += 1;
        }
        if dash.is_none() || d.len() == 0 || d.len() > usize::MAX / 2 || lt(pat_sum, 1e-6) {
            let mut i: usize = 0;
            #[cfg_attr(verus_keep_ghost, verus_spec(
                invariant
                    self.wf(),
                    self.width() == old(self).width(),
                    self.height() == old(self).height(),
                    p@.len() >= 2,
                decreases p.len() - i,
            ))]
            while i < p.len() - 1 {
                self.capsule(p[i].x, p[i].y, p[i + 1].x, p[i + 1].y, r);
                i += 1;
            }
            return;
        }
        let n = if d.len() % 2 == 1 {
            d.len() * 2
        } else {
            d.len()
        };
        let mut idx: usize = 0;
        let mut rem = fmax(d[0], 0.0);
        let mut on = true;
        let mut i: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                self.wf(),
                self.width() == old(self).width(),
                self.height() == old(self).height(),
                p@.len() >= 2,
                1 <= d@.len(),
                n == d@.len() || n == 2 * d@.len(),
                idx < n,
            decreases p.len() - i,
        ))]
        while i < p.len() - 1 {
            let ax = p[i].x;
            let ay = p[i].y;
            let sx = sub(p[i + 1].x, ax);
            let sy = sub(p[i + 1].y, ay);
            let len = sqrt(add(mul(sx, sx), mul(sy, sy)));
            let mut pos = 0.0;
            #[cfg_attr(verus_keep_ghost, verus_spec(
                invariant
                    self.wf(),
                    self.width() == old(self).width(),
                    self.height() == old(self).height(),
                    1 <= d@.len(),
                    n == d@.len() || n == 2 * d@.len(),
                    idx < n,
            ))]
            while lt(pos, len) {
                let take = fmin(rem, sub(len, pos));
                if on {
                    let t0 = div(pos, len);
                    let t1 = div(add(pos, take), len);
                    self.capsule(
                        add(ax, mul(sx, t0)),
                        add(ay, mul(sy, t0)),
                        add(ax, mul(sx, t1)),
                        add(ay, mul(sy, t1)),
                        r,
                    );
                }
                pos = add(pos, take);
                rem = sub(rem, take);
                if le(rem, 1e-9) {
                    idx = (idx + 1) % n;
                    rem = fmax(d[idx % d.len()], 0.0);
                    on = !on;
                }
            }
            i += 1;
        }
    }

    /// Even-odd fill of the closed edges (pixel space) into the coverage scratch: 8 sub-scanlines
    /// per row with exact horizontal coverage, crossings sorted with the verified stable sort.
    #[cfg_attr(verus_keep_ghost, verus_spec(
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    ))]
    pub fn fill(&mut self, es: &[Edge]) {
        if es.len() == 0 {
            return;
        }
        let w = self.w;
        let h = self.h;
        let mut ymin = div(1.0, 0.0);
        let mut ymax = div(-1.0, 0.0);
        let mut k: usize = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                k <= es@.len(),
            decreases es.len() - k,
        ))]
        while k < es.len() {
            ymin = fmin(ymin, fmin(es[k].y0, es[k].y1));
            ymax = fmax(ymax, fmax(es[k].y0, es[k].y1));
            k += 1;
        }
        let hf = from_usize(h);
        if !(lt(0.0, ymax) && lt(ymin, hf)) {
            return;
        }
        let wf = from_usize(w);
        let y0 = to_index(fmax(0.0, floor(ymin)));
        let y1 = min(to_index(fmin(sub(hf, 1.0), floor(ymax))), h - 1);
        let mut y = y0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                self.wf(),
                w == self.w,
                h == self.h,
                y1 < h,
            decreases y1 + 1 - y,
        ))]
        while y <= y1 {
            let mut amin: usize = w;
            let mut amax: usize = 0;
            let mut any = false;
            let mut s: usize = 0;
            #[cfg_attr(verus_keep_ghost, verus_spec(
                invariant
                    self.wf(),
                    w == self.w,
                    h == self.h,
                    amin <= w,
                    amax <= w,
                decreases 8 - s,
            ))]
            while s < 8 {
                let sy = add(from_usize(y), div(add(from_usize(s), 0.5), 8.0));
                self.xs.clear();
                let mut k: usize = 0;
                #[cfg_attr(verus_keep_ghost, verus_spec(
                    invariant
                        self.wf(),
                        w == self.w,
                        h == self.h,
                        k <= es@.len(),
                    decreases es.len() - k,
                ))]
                while k < es.len() {
                    let e = es[k];
                    if le(e.y0, sy) != le(e.y1, sy) {
                        let x = add(
                            e.x0,
                            div(mul(sub(sy, e.y0), sub(e.x1, e.x0)), sub(e.y1, e.y0)),
                        );
                        self.xs.push(x);
                    }
                    k += 1;
                }
                if self.xs.len() >= 2 {
                    crate::sort::stable(self.xs.as_mut_slice(), |a: &f64, b: &f64| lt(*a, *b));
                    let mut i: usize = 0;
                    #[cfg_attr(verus_keep_ghost, verus_spec(
                        invariant
                            self.wf(),
                            w == self.w,
                            h == self.h,
                            amin <= w,
                            amax <= w,
                        decreases self.xs@.len() - i,
                    ))]
                    while i < self.xs.len() && self.xs.len() - i > 1 {
                        let xa = fmax(0.0, fmin(self.xs[i], wf));
                        let xb = fmax(0.0, fmin(self.xs[i + 1], wf));
                        if !le(xb, xa) {
                            let ia = min(to_index(floor(xa)), w);
                            let ib = min(to_index(floor(xb)), w);
                            if !any {
                                let mut z: usize = 0;
                                #[cfg_attr(verus_keep_ghost, verus_spec(
                                    invariant
                                        self.wf(),
                                        w == self.w,
                                        h == self.h,
                                    decreases self.acc@.len() - z,
                                ))]
                                while z < self.acc.len() {
                                    self.acc[z] = 0.0;
                                    z += 1;
                                }
                                any = true;
                            }
                            if ia < amin {
                                amin = ia;
                            }
                            if ib > amax {
                                amax = ib;
                            }
                            if ia == ib {
                                acc_add(&mut self.acc, ia, sub(xb, xa));
                            } else if ia < ib {
                                acc_add(&mut self.acc, ia, sub(from_usize(ia + 1), xa));
                                let mut x = ia + 1;
                                #[cfg_attr(verus_keep_ghost, verus_spec(
                                    invariant
                                        self.wf(),
                                        w == self.w,
                                        h == self.h,
                                        ib <= w,
                                    decreases ib - x,
                                ))]
                                while x < ib {
                                    acc_add(&mut self.acc, x, 8.0 / 8.0);
                                    x += 1;
                                }
                                acc_add(&mut self.acc, ib, sub(xb, from_usize(ib)));
                            }
                        }
                        i += 2;
                    }
                }
                s += 1;
            }
            if any {
                let last = min(amax, w - 1);
                if amin <= last {
                    #[cfg(verus_keep_ghost)]
                    proof! {
                        lemma_row(y, w, h);
                    }
                    let row = y * w;
                    let mut x = amin;
                    #[cfg_attr(verus_keep_ghost, verus_spec(
                        invariant
                            self.wf(),
                            w == self.w,
                            h == self.h,
                            y < h,
                            row + w <= w * h <= MAX_SIDE * MAX_SIDE,
                            last < w,
                        decreases last + 1 - x,
                    ))]
                    while x <= last {
                        let v = acc_byte(&self.acc, x);
                        if v > self.cov[row + x] {
                            self.cov[row + x] = v;
                        }
                        x += 1;
                    }
                    self.mark(y, amin, last);
                }
            }
            y += 1;
        }
    }
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r == if a < b { a } else { b },
))]
#[inline(always)]
fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

#[cfg(verus_keep_ghost)]
verus! {

proof fn lemma_row(y: usize, w: usize, h: usize)
    requires
        y < h,
        w <= MAX_SIDE,
        h <= MAX_SIDE,
    ensures
        y * w + w <= w * h <= MAX_SIDE * MAX_SIDE,
{
    assert(y * w + w <= w * h <= MAX_SIDE * MAX_SIDE) by (nonlinear_arith)
        requires
            y < h,
            w <= MAX_SIDE,
            h <= MAX_SIDE,
    ;
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r@.len() == n,
))]
fn fill_u8(v: u8, n: usize) -> Vec<u8> {
    let out = alloc::vec![v; n];
    #[cfg(verus_keep_ghost)]
    proof! { assert(out@.len() == n); }
    out
}
