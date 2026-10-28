use crate::sort::*;
use alloc::vec::Vec;
use vstd::prelude::*;

verus! {

/// Longest image side (raster.zig `max_side`).
pub const MAX_SIDE: usize = 8192;

/// The float functions core lacks on stable `no_std` (the caller's math module provides them).
/// Nothing about them is assumed: every index the kernels write is proven in bounds whatever they
/// return.
pub trait FloatMath {
    fn floor(x: f64) -> f64;
    fn ceil(x: f64) -> f64;
    fn sqrt(x: f64) -> f64;
    fn round(x: f64) -> f64;
}

#[derive(Clone, Copy)]
pub struct Edge {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

#[derive(Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

// ---- trusted float boundary: total Rust operations, no properties assumed ------------------------

#[verifier::external_body]
fn fmin(a: f64, b: f64) -> f64 {
    a.min(b)
}

#[verifier::external_body]
fn fmax(a: f64, b: f64) -> f64 {
    a.max(b)
}

#[verifier::external_body]
fn add(a: f64, b: f64) -> f64 {
    a + b
}

#[verifier::external_body]
fn sub(a: f64, b: f64) -> f64 {
    a - b
}

#[verifier::external_body]
fn mul(a: f64, b: f64) -> f64 {
    a * b
}

#[verifier::external_body]
fn div(a: f64, b: f64) -> f64 {
    a / b
}

#[verifier::external_body]
fn lt(a: f64, b: f64) -> bool {
    a < b
}

#[verifier::external_body]
fn le(a: f64, b: f64) -> bool {
    a <= b
}

#[verifier::external_body]
fn abs(a: f64) -> f64 {
    a.abs()
}

/// `@intFromFloat` of a value the float logic has already clamped to `[0, n]`; saturating (NaN
/// gives 0), so it is total for any input.
#[verifier::external_body]
fn to_index(x: f64) -> usize {
    x as usize
}

#[verifier::external_body]
fn to_u8(x: f64) -> u8 {
    x as u8
}

#[verifier::external_body]
fn from_usize(n: usize) -> f64 {
    n as f64
}

/// `acc[i] += (x as f32) * wgt` with raster.zig's f32 accumulator.
#[verifier::external_body]
fn acc_add(acc: &mut Vec<f32>, i: usize, x: f64)
    requires
        i < old(acc)@.len(),
    ensures
        final(acc)@.len() == old(acc)@.len(),
{
    acc[i] += x as f32 * (1.0f32 / 8.0);
}

/// `@round(clamp(acc[i], 0, 1) * 255)` as a byte (f32 arithmetic, as raster.zig).
#[verifier::external_body]
fn acc_byte<M: FloatMath>(acc: &Vec<f32>, i: usize) -> u8
    requires
        i < acc@.len(),
{
    M::round((acc[i].min(1.0).max(0.0) * 255.0) as f64) as u8
}

// ---- canvas ---------------------------------------------------------------------------------------

/// raster.zig's `Canvas`: gray8 page (255 = paper), per-item coverage scratch and per-row dirty spans.
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

    /// A white page; `None` unless `1 <= w, h <= MAX_SIDE`.
    pub fn new(w: usize, h: usize) -> (r: Option<Canvas>)
        ensures
            r matches Some(c) ==> c.wf() && c.width() == w && c.height() == h,
            r is None <==> !(1 <= w <= MAX_SIDE && 1 <= h <= MAX_SIDE),
    {
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return None;
        }
        assert(w * h <= MAX_SIDE * MAX_SIDE) by (nonlinear_arith)
            requires
                w <= MAX_SIDE,
                h <= MAX_SIDE,
        ;
        let n = w * h;
        let pix = fill_u8(255, n);
        let cov = fill_u8(0, n);
        let mut rmin: Vec<i32> = Vec::with_capacity(h);
        let mut rmax: Vec<i32> = Vec::with_capacity(h);
        let mut y: usize = 0;
        while y < h
            invariant
                y <= h,
                rmin@.len() == y,
                rmax@.len() == y,
                forall|k: int| 0 <= k < y ==> #[trigger] rmax@[k] == -1 && rmin@[k] == i32::MAX,
            decreases h - y,
        {
            rmin.push(i32::MAX);
            rmax.push(-1);
            y += 1;
        }
        let mut acc: Vec<f32> = Vec::with_capacity(w + 2);
        let mut x: usize = 0;
        while x < w + 2
            invariant
                w <= MAX_SIDE,
                x <= w + 2,
                acc@.len() == x,
            decreases w + 2 - x,
        {
            acc.push(0.0);
            x += 1;
        }
        Some(Canvas { w, h, pix, cov, rmin, rmax, acc, xs: Vec::new() })
    }

    pub fn width_px(&self) -> usize {
        self.w
    }

    pub fn height_px(&self) -> usize {
        self.h
    }

    /// The page, row 0 at the top.
    pub fn pixels(&self) -> &[u8] {
        self.pix.as_slice()
    }

    pub fn into_pixels(self) -> Vec<u8> {
        self.pix
    }

    fn mark(&mut self, y: usize, x0: usize, x1: usize)
        requires
            old(self).wf(),
            y < old(self).h,
            x0 <= x1 < old(self).w,
        ensures
            final(self).wf(),
            final(self).w == old(self).w,
            final(self).h == old(self).h,
    {
        if (x0 as i32) < self.rmin[y] {
            self.rmin[y] = x0 as i32;
        }
        if (x1 as i32) > self.rmax[y] {
            self.rmax[y] = x1 as i32;
        }
    }

    /// Composites the coverage scratch onto the page (`dst *= 1 - coverage`) and clears it.
    pub fn flush(&mut self)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    {
        let w = self.w;
        let h = self.h;
        let mut y: usize = 0;
        while y < h
            invariant
                self.wf(),
                w == self.w,
                h == self.h,
                y <= h,
            decreases h - y,
        {
            if self.rmax[y] >= 0 {
                let x0 = self.rmin[y] as usize;
                let x1 = self.rmax[y] as usize;
                proof {
                    lemma_row(y, w, h);
                }
                let row = y * w;
                let mut x = x0;
                while x <= x1
                    invariant
                        self.wf(),
                        w == self.w,
                        h == self.h,
                        y < h,
                        row + w <= w * h <= MAX_SIDE * MAX_SIDE,
                        x0 <= x,
                        x1 < w,
                    decreases x1 + 1 - x,
                {
                    let cv = self.cov[row + x] as u32;
                    if cv != 0 {
                        self.cov[row + x] = 0;
                        let d = self.pix[row + x] as u32;
                        assert(d * (255 - cv) + 127 <= 255 * 255 + 127) by (nonlinear_arith)
                            requires
                                d <= 255,
                                cv <= 255,
                        ;
                        assert((d * (255 - cv) + 127) / 255 <= 255) by (nonlinear_arith)
                            requires
                                d <= 255,
                                cv <= 255,
                        ;
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
    pub fn capsule<M: FloatMath>(&mut self, ax: f64, ay: f64, bx: f64, by: f64, r: f64)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    {
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
            return ;
        }
        let ry0 = to_index(fmax(0.0, M::floor(sub(miny, 0.5))));
        let ry1 = min(to_index(fmin(sub(hf, 1.0), M::ceil(sub(maxy, 0.5)))), h - 1);
        let dx = sub(bx, ax);
        let dy = sub(by, ay);
        let len2 = add(mul(dx, dx), mul(dy, dy));
        let len = M::sqrt(len2);
        let has_dir = lt(1e-12, len2);
        let inv2 = if has_dir { div(1.0, len2) } else { 0.0 };
        let nx = if has_dir { div(sub(0.0, dy), len) } else { 0.0 };
        let ny = if has_dir { div(dx, len) } else { 1.0 };
        let mut y = ry0;
        while y <= ry1
            invariant
                self.wf(),
                w == self.w,
                h == self.h,
                ry1 < h,
            decreases ry1 + 1 - y,
        {
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
                let fx0 = fmax(0.0, M::floor(sub(xl, 0.5)));
                let fx1 = fmin(sub(wf, 1.0), M::ceil(sub(xr, 0.5)));
                if !lt(fx1, fx0) {
                    let ix0 = to_index(fx0);
                    let ix1 = min(to_index(fx1), w - 1);
                    if ix0 <= ix1 {
                        proof {
                            lemma_row(y, w, h);
                        }
                        let row = y * w;
                        let mut x = ix0;
                        while x <= ix1
                            invariant
                                self.wf(),
                                w == self.w,
                                h == self.h,
                                y < h,
                                row + w <= w * h <= MAX_SIDE * MAX_SIDE,
                                ix1 < w,
                            decreases ix1 + 1 - x,
                        {
                            let pxc = add(from_usize(x), 0.5);
                            let mut t = 0.0;
                            if has_dir {
                                t = mul(add(mul(sub(pxc, ax), dx), mul(sub(yc, ay), dy)), inv2);
                                t = fmax(0.0, fmin(t, 1.0));
                            }
                            let qx = sub(add(ax, mul(t, dx)), pxc);
                            let qy = sub(add(ay, mul(t, dy)), yc);
                            let cv = sub(rr, M::sqrt(add(mul(qx, qx), mul(qy, qy))));
                            if !le(cv, 0.0) {
                                let v = to_u8(M::round(mul(fmin(cv, 1.0), 255.0)));
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
    #[verifier::exec_allows_no_decreases_clause]
    pub fn stroke<M: FloatMath>(&mut self, p: &[Point], r: f64, dash: Option<&[f64]>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    {
        if p.len() == 0 {
            return ;
        }
        if p.len() == 1 {
            self.capsule::<M>(p[0].x, p[0].y, p[0].x, p[0].y, r);
            return ;
        }
        let mut pat_sum = 0.0;
        let d: &[f64] = match dash {
            Some(d) => d,
            None => &[],
        };
        let mut k: usize = 0;
        while k < d.len()
            invariant
                k <= d@.len(),
            decreases d.len() - k,
        {
            pat_sum = add(pat_sum, fmax(d[k], 0.0));
            k += 1;
        }
        if dash.is_none() || d.len() == 0 || d.len() > usize::MAX / 2 || lt(pat_sum, 1e-6) {
            let mut i: usize = 0;
            while i < p.len() - 1
                invariant
                    self.wf(),
                    self.width() == old(self).width(),
                    self.height() == old(self).height(),
                    p@.len() >= 2,
                decreases p.len() - i,
            {
                self.capsule::<M>(p[i].x, p[i].y, p[i + 1].x, p[i + 1].y, r);
                i += 1;
            }
            return ;
        }
        let n = if d.len() % 2 == 1 { d.len() * 2 } else { d.len() };
        let mut idx: usize = 0;
        let mut rem = fmax(d[0], 0.0);
        let mut on = true;
        let mut i: usize = 0;
        while i < p.len() - 1
            invariant
                self.wf(),
                self.width() == old(self).width(),
                self.height() == old(self).height(),
                p@.len() >= 2,
                1 <= d@.len(),
                n == d@.len() || n == 2 * d@.len(),
                idx < n,
            decreases p.len() - i,
        {
            let ax = p[i].x;
            let ay = p[i].y;
            let sx = sub(p[i + 1].x, ax);
            let sy = sub(p[i + 1].y, ay);
            let len = M::sqrt(add(mul(sx, sx), mul(sy, sy)));
            let mut pos = 0.0;
            while lt(pos, len)
                invariant
                    self.wf(),
                    self.width() == old(self).width(),
                    self.height() == old(self).height(),
                    1 <= d@.len(),
                    n == d@.len() || n == 2 * d@.len(),
                    idx < n,
            {
                let take = fmin(rem, sub(len, pos));
                if on {
                    let t0 = div(pos, len);
                    let t1 = div(add(pos, take), len);
                    self.capsule::<M>(add(ax, mul(sx, t0)), add(ay, mul(sy, t0)), add(ax, mul(sx, t1)), add(ay, mul(sy, t1)), r);
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
    pub fn fill<M: FloatMath>(&mut self, es: &[Edge])
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).width() == old(self).width(),
            final(self).height() == old(self).height(),
    {
        if es.len() == 0 {
            return ;
        }
        let w = self.w;
        let h = self.h;
        let mut ymin = div(1.0, 0.0);
        let mut ymax = div(-1.0, 0.0);
        let mut k: usize = 0;
        while k < es.len()
            invariant
                k <= es@.len(),
            decreases es.len() - k,
        {
            ymin = fmin(ymin, fmin(es[k].y0, es[k].y1));
            ymax = fmax(ymax, fmax(es[k].y0, es[k].y1));
            k += 1;
        }
        let hf = from_usize(h);
        if !(lt(0.0, ymax) && lt(ymin, hf)) {
            return ;
        }
        let wf = from_usize(w);
        let y0 = to_index(fmax(0.0, M::floor(ymin)));
        let y1 = min(to_index(fmin(sub(hf, 1.0), M::floor(ymax))), h - 1);
        let mut y = y0;
        while y <= y1
            invariant
                self.wf(),
                w == self.w,
                h == self.h,
                y1 < h,
            decreases y1 + 1 - y,
        {
            let mut amin: usize = w;
            let mut amax: usize = 0;
            let mut any = false;
            let mut s: usize = 0;
            while s < 8
                invariant
                    self.wf(),
                    w == self.w,
                    h == self.h,
                    amin <= w,
                    amax <= w,
                decreases 8 - s,
            {
                let sy = add(from_usize(y), div(add(from_usize(s), 0.5), 8.0));
                self.xs.clear();
                let mut k: usize = 0;
                while k < es.len()
                    invariant
                        self.wf(),
                        w == self.w,
                        h == self.h,
                        k <= es@.len(),
                    decreases es.len() - k,
                {
                    let e = es[k];
                    if le(e.y0, sy) != le(e.y1, sy) {
                        let x = add(e.x0, div(mul(sub(sy, e.y0), sub(e.x1, e.x0)), sub(e.y1, e.y0)));
                        self.xs.push(x);
                    }
                    k += 1;
                }
                if self.xs.len() >= 2 {
                    stable_sort_by(self.xs.as_mut_slice(), |a: &f64, b: &f64| lt(*a, *b));
                    let mut i: usize = 0;
                    while i < self.xs.len() && self.xs.len() - i > 1
                        invariant
                            self.wf(),
                            w == self.w,
                            h == self.h,
                            amin <= w,
                            amax <= w,
                        decreases self.xs@.len() - i,
                    {
                        let xa = fmax(0.0, fmin(self.xs[i], wf));
                        let xb = fmax(0.0, fmin(self.xs[i + 1], wf));
                        if !le(xb, xa) {
                            let ia = min(to_index(M::floor(xa)), w);
                            let ib = min(to_index(M::floor(xb)), w);
                            if !any {
                                let mut z: usize = 0;
                                while z < self.acc.len()
                                    invariant
                                        self.wf(),
                                        w == self.w,
                                        h == self.h,
                                    decreases self.acc@.len() - z,
                                {
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
                                while x < ib
                                    invariant
                                        self.wf(),
                                        w == self.w,
                                        h == self.h,
                                        ib <= w,
                                    decreases ib - x,
                                {
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
                    proof {
                        lemma_row(y, w, h);
                    }
                    let row = y * w;
                    let mut x = amin;
                    while x <= last
                        invariant
                            self.wf(),
                            w == self.w,
                            h == self.h,
                            y < h,
                            row + w <= w * h <= MAX_SIDE * MAX_SIDE,
                            last < w,
                        decreases last + 1 - x,
                    {
                        let v = acc_byte::<M>(&self.acc, x);
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

fn min(a: usize, b: usize) -> (r: usize)
    ensures
        r == if a < b { a } else { b },
{
    if a < b { a } else { b }
}

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

fn fill_u8(v: u8, n: usize) -> (r: Vec<u8>)
    ensures
        r@.len() == n,
{
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut i: usize = 0;
    while i < n
        invariant
            i <= n,
            out@.len() == i,
        decreases n - i,
    {
        out.push(v);
        i += 1;
    }
    out
}

} // verus!
