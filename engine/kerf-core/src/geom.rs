//! 2D geometry primitives: vectors, bulge arcs, loops, transforms, robust orientation, and segment
//! intersection. Loops are closed polylines of `Pt` (x, y, bulge); the bulge on vertex i describes the
//! segment i -> i+1 with DXF LWPOLYLINE semantics (bulge = tan(theta/4), CCW positive).

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::math;
use crate::num;
use alloc::vec::Vec;
use core::f64::consts::PI;

/// Radians per degree and back, as full-precision literals.
pub const RAD_PER_DEG: f64 = 0.017453292519943295769236907684886127134428718885417;
pub const DEG_PER_RAD: f64 = 57.295779513082320876798154814105170332405472466564;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

impl V2 {
    pub fn init(x: f64, y: f64) -> V2 {
        V2 { x, y }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, b: V2) -> V2 {
        V2 { x: self.x + b.x, y: self.y + b.y }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, b: V2) -> V2 {
        V2 { x: self.x - b.x, y: self.y - b.y }
    }
    pub fn scale(self, s: f64) -> V2 {
        V2 { x: self.x * s, y: self.y * s }
    }
    pub fn dot(self, b: V2) -> f64 {
        self.x * b.x + self.y * b.y
    }
    pub fn cross(self, b: V2) -> f64 {
        self.x * b.y - self.y * b.x
    }
    pub fn len(self) -> f64 {
        math::sqrt(self.x * self.x + self.y * self.y)
    }
    pub fn dist(self, b: V2) -> f64 {
        self.sub(b).len()
    }
    pub fn norm(self) -> V2 {
        let l = self.len();
        if l == 0.0 {
            return self;
        }
        V2 { x: self.x / l, y: self.y / l }
    }
    /// Left normal (-y, x).
    pub fn perp(self) -> V2 {
        V2 { x: -self.y, y: self.x }
    }
    pub fn lerp(self, b: V2, t: f64) -> V2 {
        V2 { x: self.x + (b.x - self.x) * t, y: self.y + (b.y - self.y) * t }
    }
    pub fn mid(self, b: V2) -> V2 {
        self.lerp(b, 0.5)
    }
    pub fn eql(self, b: V2, tol: f64) -> bool {
        (self.x - b.x).abs() <= tol && (self.y - b.y).abs() <= tol
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub b: f64,
}

impl Pt {
    pub fn new(x: f64, y: f64) -> Pt {
        Pt { x, y, b: 0.0 }
    }
    pub fn v(self) -> V2 {
        V2 { x: self.x, y: self.y }
    }
    pub fn at(p: V2, b: f64) -> Pt {
        Pt { x: p.x, y: p.y, b }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Default for Box {
    fn default() -> Box {
        Box { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY }
    }
}

impl Box {
    pub fn is_empty(&self) -> bool {
        self.x0 > self.x1 || self.y0 > self.y1
    }
    pub fn add_point(&mut self, x: f64, y: f64) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }
    pub fn add_box(&mut self, o: Box) {
        if o.is_empty() {
            return;
        }
        self.add_point(o.x0, o.y0);
        self.add_point(o.x1, o.y1);
    }
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn overlaps(&self, o: Box, tol: f64) -> bool {
        self.x0 <= o.x1 + tol && o.x0 <= self.x1 + tol && self.y0 <= o.y1 + tol && o.y0 <= self.y1 + tol
    }
    pub fn contains(&self, p: V2) -> bool {
        p.x >= self.x0 && p.x <= self.x1 && p.y >= self.y0 && p.y <= self.y1
    }
    pub fn center(&self) -> V2 {
        V2 { x: (self.x0 + self.x1) / 2.0, y: (self.y0 + self.y1) / 2.0 }
    }
    pub fn expand(&self, d: f64) -> Box {
        Box { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
}

// ---- affine transforms ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Xf {
    fn default() -> Xf {
        Xf::IDENTITY
    }
}

impl Xf {
    pub const IDENTITY: Xf = Xf { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    pub fn apply(&self, p: V2) -> V2 {
        V2 { x: self.a * p.x + self.c * p.y + self.tx, y: self.b * p.x + self.d * p.y + self.ty }
    }
    pub fn apply_dir(&self, p: V2) -> V2 {
        V2 { x: self.a * p.x + self.c * p.y, y: self.b * p.x + self.d * p.y }
    }
    pub fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    /// `self` applied after `n`: (m * n)(p) = m(n(p)).
    pub fn mul(&self, n: Xf) -> Xf {
        let m = self;
        Xf {
            a: m.a * n.a + m.c * n.b,
            b: m.b * n.a + m.d * n.b,
            c: m.a * n.c + m.c * n.d,
            d: m.b * n.c + m.d * n.d,
            tx: m.a * n.tx + m.c * n.ty + m.tx,
            ty: m.b * n.tx + m.d * n.ty + m.ty,
        }
    }
    pub fn translate(x: f64, y: f64) -> Xf {
        Xf { tx: x, ty: y, ..Xf::IDENTITY }
    }
    pub fn rotate(rad: f64) -> Xf {
        let c = math::cos(rad);
        let s = math::sin(rad);
        Xf { a: c, b: s, c: -s, d: c, tx: 0.0, ty: 0.0 }
    }
    /// Rotate by `rad` about point `p`.
    pub fn rotate_about(rad: f64, p: V2) -> Xf {
        Xf::translate(p.x, p.y).mul(Xf::rotate(rad)).mul(Xf::translate(-p.x, -p.y))
    }
    pub fn scaling(sx: f64, sy: f64) -> Xf {
        Xf { a: sx, d: sy, ..Xf::IDENTITY }
    }
    pub fn is_identity(&self) -> bool {
        *self == Xf::IDENTITY
    }
    pub fn apply_pt(&self, p: Pt) -> Pt {
        let q = self.apply(p.v());
        Pt { x: q.x, y: q.y, b: if self.det() < 0.0 { -p.b } else { p.b } }
    }
    pub fn apply_loop(&self, lp: &[Pt]) -> Vec<Pt> {
        lp.iter().map(|&p| self.apply_pt(p)).collect()
    }
}

// ---- arcs -----------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Arc {
    pub c: V2,
    pub r: f64,
    pub a0: f64,
    /// Signed sweep in radians (CCW positive).
    pub sweep: f64,
}

impl Arc {
    pub fn at(&self, t: f64) -> V2 {
        let a = self.a0 + self.sweep * t;
        V2 { x: self.c.x + self.r * math::cos(a), y: self.c.y + self.r * math::sin(a) }
    }
}

/// Arc through p0 -> p1 with the given non-zero bulge.
pub fn arc_of(p0: V2, p1: V2, bulge: f64) -> Arc {
    let chord = p1.sub(p0);
    let cl = chord.len();
    let sweep = 4.0 * math::atan(bulge);
    let r = cl * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let d = (cl / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    let m = p0.mid(p1);
    let n = if cl > 0.0 { chord.perp().scale(1.0 / cl) } else { V2::init(0.0, 0.0) };
    let c = m.add(n.scale(d));
    Arc { c, r, a0: math::atan2(p0.y - c.y, p0.x - c.x), sweep }
}

pub fn bulge_from_sweep(sweep: f64) -> f64 {
    math::tan(sweep / 4.0)
}

#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub a: V2,
    pub b: V2,
    pub bulge: f64,
}

/// Sub-segment of (p0, p1, bulge) between parameters t0 < t1 (parameter = fraction of the sweep or chord).
pub fn sub_seg(p0: V2, p1: V2, bulge: f64, t0: f64, t1: f64) -> Seg {
    if bulge == 0.0 {
        return Seg { a: p0.lerp(p1, t0), b: p0.lerp(p1, t1), bulge: 0.0 };
    }
    let arc = arc_of(p0, p1, bulge);
    let a = if t0 <= 0.0 { p0 } else { arc.at(t0) };
    let b = if t1 >= 1.0 { p1 } else { arc.at(t1) };
    Seg { a, b, bulge: bulge_from_sweep(arc.sweep * (t1 - t0)) }
}

/// Point on the segment at parameter t.
pub fn seg_point(p0: V2, p1: V2, bulge: f64, t: f64) -> V2 {
    if bulge == 0.0 {
        return p0.lerp(p1, t);
    }
    if t <= 0.0 {
        return p0;
    }
    if t >= 1.0 {
        return p1;
    }
    arc_of(p0, p1, bulge).at(t)
}

/// Append the flattened points of segment p0 -> p1 (excluding p0, including p1). Chord sagitta <= tol.
pub fn flatten_seg_into(list: &mut Vec<V2>, p0: V2, p1: V2, bulge: f64, tol: f64) {
    if bulge == 0.0 {
        list.push(p1);
        return;
    }
    let arc = arc_of(p0, p1, bulge);
    let n = arc_steps(arc.r, arc.sweep, tol);
    for i in 1..n {
        list.push(arc.at(i as f64 / n as f64));
    }
    list.push(p1);
}

/// Upper bound on the segments of one flattened arc.
pub const MAX_ARC_STEPS: usize = 4096;

/// Flattening tolerance of arcs for 2D section geometry and validation, model inches.
pub const FLAT_TOL: f64 = 0.005;

/// Segments for an arc of `sweep` radians when one segment may span at most `step` radians; clamped to [2, MAX_ARC_STEPS].
pub fn steps_for_sweep(sweep: f64, step: f64) -> usize {
    num::to_int_clamped(math::ceil(sweep.abs() / step), 2usize, MAX_ARC_STEPS)
}

pub fn arc_steps(r: f64, sweep: f64, tol: f64) -> usize {
    if !(r > tol) {
        return 2;
    }
    let step = 2.0 * math::acos(1.0 - tol / r);
    if !(step > 0.0) {
        return 2;
    }
    steps_for_sweep(sweep, step)
}

pub fn flatten_loop(lp: &[Pt], tol: f64) -> Vec<V2> {
    let mut out = Vec::new();
    let n = lp.len();
    for (i, p) in lp.iter().enumerate() {
        let q = lp[(i + 1) % n];
        out.push(p.v());
        if p.b != 0.0 {
            let mut tmp = Vec::new();
            flatten_seg_into(&mut tmp, p.v(), q.v(), p.b, tol);
            if let Some((_, mids)) = tmp.split_last() {
                out.extend_from_slice(mids);
            }
        }
    }
    out
}

/// Polyline flatten for open or closed pts (closed adds the wrap segment).
pub fn flatten_polyline(pts: &[Pt], closed: bool, tol: f64) -> Vec<V2> {
    let mut out = Vec::new();
    let Some(first) = pts.first() else { return out };
    out.push(first.v());
    let n = if closed { pts.len() } else { pts.len() - 1 };
    for i in 0..n {
        let q = pts[(i + 1) % pts.len()];
        flatten_seg_into(&mut out, pts[i].v(), q.v(), pts[i].b, tol);
    }
    out
}

/// Signed area of a closed bulge loop (CCW positive), exact for arcs.
pub fn signed_area(lp: &[Pt]) -> f64 {
    let n = lp.len();
    let mut s = 0.0;
    for (i, p) in lp.iter().enumerate() {
        let q = lp[(i + 1) % n];
        s += p.x * q.y - q.x * p.y;
        if p.b != 0.0 {
            let arc = arc_of(p.v(), q.v(), p.b);
            let th = arc.sweep;
            s += arc.r * arc.r * (th - math::sin(th));
        }
    }
    s / 2.0
}

pub fn signed_area_v(pts: &[V2]) -> f64 {
    let n = pts.len();
    let mut s = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let q = pts[(i + 1) % n];
        s += p.x * q.y - q.x * p.y;
    }
    s / 2.0
}

pub fn loop_box(lp: &[Pt]) -> Box {
    let mut b = Box::default();
    for (i, p) in lp.iter().enumerate() {
        b.add_point(p.x, p.y);
        if p.b != 0.0 {
            let q = lp[(i + 1) % lp.len()];
            seg_box_into(&mut b, p.v(), q.v(), p.b);
        }
    }
    b
}

/// Extend `b` by the extremes of an arc segment.
pub fn seg_box_into(b: &mut Box, p0: V2, p1: V2, bulge: f64) {
    b.add_point(p0.x, p0.y);
    b.add_point(p1.x, p1.y);
    if bulge == 0.0 {
        return;
    }
    let arc = arc_of(p0, p1, bulge);
    for k in 0..4 {
        let ang = k as f64 * PI / 2.0;
        if angle_in_sweep(ang, arc.a0, arc.sweep) {
            b.add_point(arc.c.x + arc.r * math::cos(ang), arc.c.y + arc.r * math::sin(ang));
        }
    }
}

/// Floored float modulus (the result has the sign of `b`).
fn fmod_floor(a: f64, b: f64) -> f64 {
    if a < 0.0 { ((a % b) + b) % b } else { a % b }
}

pub fn angle_in_sweep(ang: f64, a0: f64, sweep: f64) -> bool {
    let two_pi = 2.0 * PI;
    let d = ang - a0;
    if sweep >= 0.0 { fmod_floor(d, two_pi) <= sweep + 1e-12 } else { fmod_floor(-d, two_pi) <= -sweep + 1e-12 }
}

pub fn points_box(pts: &[Pt]) -> Box {
    let mut b = Box::default();
    for p in pts {
        b.add_point(p.x, p.y);
    }
    b
}

/// Reversing a bulge loop: vertex order reverses and segment i (v_i -> v_{i+1}, bulge b_i) becomes
/// segment v_{i+1} -> v_i with bulge -b_i.
pub fn reverse_loop(lp: &[Pt]) -> Vec<Pt> {
    let n = lp.len();
    (0..n)
        .map(|i| {
            let src = n - 1 - i;
            let prev = (src + n - 1) % n;
            Pt { x: lp[src].x, y: lp[src].y, b: -lp[prev].b }
        })
        .collect()
}

// ---- robust orientation ---------------------------------------------------------------------------

fn two_sum(a: f64, b: f64) -> [f64; 2] {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    [s, e]
}

/// Exact product error by Dekker/Veltkamp splitting (what a fused multiply-add gives, without needing one).
fn two_prod(a: f64, b: f64) -> [f64; 2] {
    const SPLIT: f64 = 134217729.0;
    let p = a * b;
    let split = |x: f64| {
        let c = SPLIT * x;
        let hi = c - (c - x);
        (hi, x - hi)
    };
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    let e = ((ah * bh - p) + ah * bl + al * bh) + al * bl;
    [p, e]
}

struct Exp {
    v: [f64; 24],
    n: usize,
}

impl Exp {
    fn grow(&mut self, b: f64) {
        let mut q = b;
        let mut k = 0;
        for i in 0..self.n {
            let s = two_sum(q, self.v[i]);
            q = s[0];
            if s[1] != 0.0 {
                self.v[k] = s[1];
                k += 1;
            }
        }
        if q != 0.0 || k == 0 {
            self.v[k] = q;
            k += 1;
        }
        self.n = k;
    }
    fn sign(&self) -> i32 {
        for i in (0..self.n).rev() {
            if self.v[i] > 0.0 {
                return 1;
            }
            if self.v[i] < 0.0 {
                return -1;
            }
        }
        0
    }
}

/// Sign of the orientation determinant: +1 if c is left of a->b, -1 right, 0 collinear (exact).
pub fn orient2d(a: V2, b: V2, c: V2) -> i32 {
    let detleft = (a.x - c.x) * (b.y - c.y);
    let detright = (a.y - c.y) * (b.x - c.x);
    let det = detleft - detright;
    let bound = 3.3306690738754716e-16 * (detleft.abs() + detright.abs());
    if det > bound {
        return 1;
    }
    if det < -bound {
        return -1;
    }
    let dx1 = two_sum(a.x, -c.x);
    let dy1 = two_sum(b.y, -c.y);
    let dy2 = two_sum(a.y, -c.y);
    let dx2 = two_sum(b.x, -c.x);
    let mut e = Exp { v: [0.0; 24], n: 0 };
    for i in 0..2 {
        for j in 0..2 {
            let p = two_prod(dx1[i], dy1[j]);
            e.grow(p[1]);
            e.grow(p[0]);
            let q = two_prod(dy2[i], dx2[j]);
            e.grow(-q[1]);
            e.grow(-q[0]);
        }
    }
    e.sign()
}

// ---- point location -------------------------------------------------------------------------------

pub fn dist_point_seg(p: V2, a: V2, b: V2) -> f64 {
    let ab = b.sub(a);
    let l2 = ab.dot(ab);
    if l2 == 0.0 {
        return p.dist(a);
    }
    let t = (p.sub(a).dot(ab) / l2).clamp(0.0, 1.0);
    p.dist(a.lerp(b, t))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    Outside,
    Inside,
    Boundary,
}

/// Winding-number point location against flattened loops (CCW filled / CW hole). `tol` is the boundary band.
pub fn locate<L: AsRef<[V2]>>(p: V2, loops: &[L], tol: f64) -> Loc {
    let mut wn = 0i32;
    for lp in loops {
        let lp = lp.as_ref();
        for (i, &a) in lp.iter().enumerate() {
            let b = lp[(i + 1) % lp.len()];
            if dist_point_seg(p, a, b) <= tol {
                return Loc::Boundary;
            }
            if a.y <= p.y {
                if b.y > p.y && orient2d(a, b, p) > 0 {
                    wn += 1;
                }
            } else if b.y <= p.y && orient2d(a, b, p) < 0 {
                wn -= 1;
            }
        }
    }
    if wn != 0 { Loc::Inside } else { Loc::Outside }
}

/// Even-odd variant for sets whose orientation is unspecified.
pub fn locate_even_odd<L: AsRef<[V2]>>(p: V2, loops: &[L], tol: f64) -> Loc {
    let mut inside = false;
    for lp in loops {
        let lp = lp.as_ref();
        for (i, &a) in lp.iter().enumerate() {
            let b = lp[(i + 1) % lp.len()];
            if dist_point_seg(p, a, b) <= tol {
                return Loc::Boundary;
            }
            if (a.y > p.y) != (b.y > p.y) {
                let xint = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < xint {
                    inside = !inside;
                }
            }
        }
    }
    if inside { Loc::Inside } else { Loc::Outside }
}

pub fn point_in_loop_eo(p: V2, lp: &[V2]) -> bool {
    let mut inside = false;
    for (i, &a) in lp.iter().enumerate() {
        let b = lp[(i + 1) % lp.len()];
        if (a.y > p.y) != (b.y > p.y) {
            let xint = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < xint {
                inside = !inside;
            }
        }
    }
    inside
}

pub fn centroid_v(pts: &[V2]) -> V2 {
    let mut a = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let q = pts[(i + 1) % pts.len()];
        let w = p.x * q.y - q.x * p.y;
        a += w;
        cx += (p.x + q.x) * w;
        cy += (p.y + q.y) * w;
    }
    if a.abs() < 1e-12 {
        let mut sx = 0.0;
        let mut sy = 0.0;
        for p in pts {
            sx += p.x;
            sy += p.y;
        }
        let n = pts.len().max(1) as f64;
        return V2 { x: sx / n, y: sy / n };
    }
    V2 { x: cx / (3.0 * a), y: cy / (3.0 * a) }
}

// ---- segment intersection -------------------------------------------------------------------------

/// Intersection parameters of two straight segments. Fills `ta`/`tb` (positions along a and b in
/// [0,1]) and returns the number of intersection points (0, 1, or 2 for collinear overlap).
pub fn seg_seg(a0: V2, a1: V2, b0: V2, b1: V2, ta: &mut [f64; 2], tb: &mut [f64; 2]) -> usize {
    let d1 = orient2d(b0, b1, a0);
    let d2 = orient2d(b0, b1, a1);
    let d3 = orient2d(a0, a1, b0);
    let d4 = orient2d(a0, a1, b1);
    if d1 == 0 && d2 == 0 && d3 == 0 && d4 == 0 {
        let ab = a1.sub(a0);
        let l2 = ab.dot(ab);
        if l2 == 0.0 {
            return 0;
        }
        let tb0 = b0.sub(a0).dot(ab) / l2;
        let tb1 = b1.sub(a0).dot(ab) / l2;
        let lo = tb0.min(tb1).max(0.0);
        let hi = tb0.max(tb1).min(1.0);
        if lo > hi {
            return 0;
        }
        let bb = b1.sub(b0);
        let bl2 = bb.dot(bb);
        ta[0] = lo;
        ta[1] = hi;
        if bl2 == 0.0 {
            tb[0] = 0.0;
            tb[1] = 0.0;
        } else {
            tb[0] = (a0.add(ab.scale(lo)).sub(b0).dot(bb) / bl2).clamp(0.0, 1.0);
            tb[1] = (a0.add(ab.scale(hi)).sub(b0).dot(bb) / bl2).clamp(0.0, 1.0);
        }
        return if hi - lo < 1e-15 { 1 } else { 2 };
    }
    if d1 * d2 > 0 || d3 * d4 > 0 {
        return 0;
    }
    let r = a1.sub(a0);
    let s = b1.sub(b0);
    let denom = r.cross(s);
    if denom == 0.0 {
        return 0;
    }
    let w = b0.sub(a0);
    let mut t = w.cross(s) / denom;
    let mut u = w.cross(r) / denom;
    if d1 == 0 && d2 != 0 {
        t = 0.0;
    }
    if d2 == 0 && d1 != 0 {
        t = 1.0;
    }
    if d3 == 0 && d4 != 0 {
        u = 0.0;
    }
    if d4 == 0 && d3 != 0 {
        u = 1.0;
    }
    ta[0] = t.clamp(0.0, 1.0);
    tb[0] = u.clamp(0.0, 1.0);
    1
}

/// Parameters t (fraction of the sweep) at which the arc (p0,p1,bulge) meets the straight segment
/// b0->b1, with the matching parameter u on the straight segment. Up to 2 hits.
pub fn arc_seg(p0: V2, p1: V2, bulge: f64, b0: V2, b1: V2, ta: &mut [f64; 2], tb: &mut [f64; 2]) -> usize {
    let arc = arc_of(p0, p1, bulge);
    let d = b1.sub(b0);
    let f = b0.sub(arc.c);
    let aa = d.dot(d);
    if aa == 0.0 {
        return 0;
    }
    let bq = 2.0 * f.dot(d);
    let cq = f.dot(f) - arc.r * arc.r;
    let disc = bq * bq - 4.0 * aa * cq;
    if disc < 0.0 && !(disc > -1e-18 * aa * arc.r * arc.r) {
        return 0;
    }
    let sq = math::sqrt(disc.max(0.0));
    let mut n = 0;
    let roots = [(-bq - sq) / (2.0 * aa), (-bq + sq) / (2.0 * aa)];
    for (k, &u) in roots.iter().enumerate() {
        if k == 1 && sq == 0.0 {
            break;
        }
        if !(-1e-12..=1.0 + 1e-12).contains(&u) {
            continue;
        }
        let uc = u.clamp(0.0, 1.0);
        let pt = b0.lerp(b1, uc);
        let ang = math::atan2(pt.y - arc.c.y, pt.x - arc.c.x);
        let mut da = ang - arc.a0;
        let two_pi = 2.0 * PI;
        if arc.sweep >= 0.0 {
            da = fmod_floor(da, two_pi);
            if da > arc.sweep + 1e-9 {
                if da - two_pi < -1e-9 {
                    continue;
                }
                da -= two_pi;
            }
        } else {
            da = -fmod_floor(-da, two_pi);
            if da < arc.sweep - 1e-9 {
                if da + two_pi > 1e-9 {
                    continue;
                }
                da += two_pi;
            }
        }
        let t = (da / arc.sweep).clamp(0.0, 1.0);
        ta[n] = t;
        tb[n] = uc;
        n += 1;
    }
    n
}
