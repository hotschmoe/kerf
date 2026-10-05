//! 2D geometry kernel: points, bulge loops, arcs, intersections, containment.

use std::f64::consts::PI;

pub const TOL: f64 = 1e-7;
pub const TAU: f64 = 2.0 * PI;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

pub fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

impl std::ops::Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        pt(self.x + o.x, self.y + o.y)
    }
}
impl std::ops::Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        pt(self.x - o.x, self.y - o.y)
    }
}
impl std::ops::Mul<f64> for Pt {
    type Output = Pt;
    fn mul(self, k: f64) -> Pt {
        pt(self.x * k, self.y * k)
    }
}
impl Pt {
    pub fn dot(self, o: Pt) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: Pt) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dist(self, o: Pt) -> f64 {
        (self - o).len()
    }
    pub fn norm(self) -> Pt {
        let l = self.len();
        if l < 1e-15 { pt(0.0, 0.0) } else { pt(self.x / l, self.y / l) }
    }
    /// Left normal (rotate +90 degrees).
    pub fn perp(self) -> Pt {
        pt(-self.y, self.x)
    }
    pub fn rot(self, ang: f64) -> Pt {
        let (s, c) = ang.sin_cos();
        pt(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    pub fn lerp(self, o: Pt, t: f64) -> Pt {
        pt(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
    pub fn near(self, o: Pt, tol: f64) -> bool {
        (self.x - o.x).abs() <= tol && (self.y - o.y).abs() <= tol
    }
}

/// Polyline vertex with DXF-style bulge for the segment to the next vertex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V {
    pub x: f64,
    pub y: f64,
    pub b: f64,
}

pub fn v(x: f64, y: f64) -> V {
    V { x, y, b: 0.0 }
}
pub fn vb(x: f64, y: f64, b: f64) -> V {
    V { x, y, b }
}
impl V {
    pub fn p(&self) -> Pt {
        pt(self.x, self.y)
    }
}

pub type Loop = Vec<V>;

#[derive(Clone, Debug, Default)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

impl Region {
    pub fn new(outer: Loop) -> Region {
        Region { outer, holes: vec![] }
    }
    pub fn loops(&self) -> impl Iterator<Item = &Loop> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }
    pub fn empty() -> Rect {
        Rect { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY }
    }
    pub fn is_empty(&self) -> bool {
        self.x0 > self.x1
    }
    pub fn add(&mut self, p: Pt) {
        self.x0 = self.x0.min(p.x);
        self.y0 = self.y0.min(p.y);
        self.x1 = self.x1.max(p.x);
        self.y1 = self.y1.max(p.y);
    }
    pub fn union(&mut self, o: &Rect) {
        if o.is_empty() {
            return;
        }
        self.add(pt(o.x0, o.y0));
        self.add(pt(o.x1, o.y1));
    }
    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }
    pub fn cx(&self) -> f64 {
        (self.x0 + self.x1) * 0.5
    }
    pub fn cy(&self) -> f64 {
        (self.y0 + self.y1) * 0.5
    }
    pub fn overlaps(&self, o: &Rect, tol: f64) -> bool {
        self.x0 <= o.x1 + tol && o.x0 <= self.x1 + tol && self.y0 <= o.y1 + tol && o.y0 <= self.y1 + tol
    }
    pub fn contains(&self, p: Pt, tol: f64) -> bool {
        p.x >= self.x0 - tol && p.x <= self.x1 + tol && p.y >= self.y0 - tol && p.y <= self.y1 + tol
    }
    pub fn inflate(&self, d: f64) -> Rect {
        Rect { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
    pub fn corners(&self) -> [Pt; 4] {
        [pt(self.x0, self.y0), pt(self.x1, self.y0), pt(self.x1, self.y1), pt(self.x0, self.y1)]
    }
    pub fn loop_(&self) -> Loop {
        vec![v(self.x0, self.y0), v(self.x1, self.y0), v(self.x1, self.y1), v(self.x0, self.y1)]
    }
    /// The 9 box anchors by name.
    pub fn anchor(&self, name: &str) -> Option<Pt> {
        let (xs, ys) = match name {
            "top_left" => (self.x0, self.y1),
            "top_center" => (self.cx(), self.y1),
            "top_right" => (self.x1, self.y1),
            "middle_left" => (self.x0, self.cy()),
            "center" => (self.cx(), self.cy()),
            "middle_right" => (self.x1, self.cy()),
            "bottom_left" => (self.x0, self.y0),
            "bottom_center" => (self.cx(), self.y0),
            "bottom_right" => (self.x1, self.y0),
            _ => return None,
        };
        Some(pt(xs, ys))
    }
}

pub const BOX_ANCHORS: [&str; 9] = [
    "top_left",
    "top_center",
    "top_right",
    "middle_left",
    "center",
    "middle_right",
    "bottom_left",
    "bottom_center",
    "bottom_right",
];

/// 2D affine transform: x' = a*x + c*y + e, y' = b*x + d*y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Xf {
    pub fn identity() -> Xf {
        Xf { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }
    pub fn translate(dx: f64, dy: f64) -> Xf {
        Xf { e: dx, f: dy, ..Xf::identity() }
    }
    pub fn rotate(ang: f64) -> Xf {
        let (s, c) = ang.sin_cos();
        Xf { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }
    /// Mirror about the vertical line x = x0.
    pub fn mirror_x(x0: f64) -> Xf {
        Xf { a: -1.0, b: 0.0, c: 0.0, d: 1.0, e: 2.0 * x0, f: 0.0 }
    }
    pub fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    pub fn apply(&self, p: Pt) -> Pt {
        pt(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }
    pub fn apply_vec(&self, p: Pt) -> Pt {
        pt(self.a * p.x + self.c * p.y, self.b * p.x + self.d * p.y)
    }
    /// self applied after `first`: result(p) = self(first(p)).
    pub fn after(&self, first: &Xf) -> Xf {
        Xf {
            a: self.a * first.a + self.c * first.b,
            b: self.b * first.a + self.d * first.b,
            c: self.a * first.c + self.c * first.d,
            d: self.b * first.c + self.d * first.d,
            e: self.a * first.e + self.c * first.f + self.e,
            f: self.b * first.e + self.d * first.f + self.f,
        }
    }
    pub fn loop_(&self, l: &Loop) -> Loop {
        let flip = self.det() < 0.0;
        l.iter()
            .map(|p| {
                let q = self.apply(p.p());
                V { x: q.x, y: q.y, b: if flip { -p.b } else { p.b } }
            })
            .collect()
    }
    pub fn region(&self, r: &Region) -> Region {
        Region { outer: self.loop_(&r.outer), holes: r.holes.iter().map(|h| self.loop_(h)).collect() }
    }
}

/// A geometric segment: line or circular arc.
#[derive(Clone, Copy, Debug)]
pub enum Seg {
    Line(Pt, Pt),
    Arc { p0: Pt, p1: Pt, c: Pt, r: f64, a0: f64, sw: f64 },
}

pub fn bulge_to_seg(p0: Pt, p1: Pt, b: f64) -> Seg {
    if b.abs() < 1e-12 || p0.dist(p1) < 1e-12 {
        return Seg::Line(p0, p1);
    }
    let l = p0.dist(p1);
    let sw = 4.0 * b.atan();
    // center lies on the left of the chord by d (signed)
    let d = (l * 0.5) * (1.0 - b * b) / (2.0 * b);
    let m = p0.lerp(p1, 0.5);
    let n = (p1 - p0).perp().norm();
    let c = m + n * d;
    let r = (l * 0.5) / (sw * 0.5).sin().abs();
    let a0 = (p0.y - c.y).atan2(p0.x - c.x);
    Seg::Arc { p0, p1, c, r, a0, sw }
}

pub fn wrap_pi(a: f64) -> f64 {
    let mut x = a % TAU;
    if x > PI {
        x -= TAU;
    } else if x < -PI {
        x += TAU;
    }
    x
}

impl Seg {
    pub fn start(&self) -> Pt {
        match *self {
            Seg::Line(a, _) => a,
            Seg::Arc { p0, .. } => p0,
        }
    }
    pub fn end(&self) -> Pt {
        match *self {
            Seg::Line(_, b) => b,
            Seg::Arc { p1, .. } => p1,
        }
    }
    pub fn len(&self) -> f64 {
        match *self {
            Seg::Line(a, b) => a.dist(b),
            Seg::Arc { r, sw, .. } => r * sw.abs(),
        }
    }
    pub fn at(&self, t: f64) -> Pt {
        match *self {
            Seg::Line(a, b) => a.lerp(b, t),
            Seg::Arc { p0, p1, c, r, a0, sw } => {
                if t <= 0.0 {
                    p0
                } else if t >= 1.0 {
                    p1
                } else {
                    let a = a0 + sw * t;
                    pt(c.x + r * a.cos(), c.y + r * a.sin())
                }
            }
        }
    }
    pub fn bulge(&self) -> f64 {
        match *self {
            Seg::Line(..) => 0.0,
            Seg::Arc { sw, .. } => (sw / 4.0).tan(),
        }
    }
    pub fn sub(&self, t0: f64, t1: f64) -> Seg {
        match *self {
            Seg::Line(..) => Seg::Line(self.at(t0), self.at(t1)),
            Seg::Arc { c, r, a0, sw, .. } => Seg::Arc { p0: self.at(t0), p1: self.at(t1), c, r, a0: a0 + sw * t0, sw: sw * (t1 - t0) },
        }
    }
    pub fn reversed(&self) -> Seg {
        match *self {
            Seg::Line(a, b) => Seg::Line(b, a),
            Seg::Arc { p0, p1, c, r, a0, sw } => Seg::Arc { p0: p1, p1: p0, c, r, a0: a0 + sw, sw: -sw },
        }
    }
    pub fn is_line(&self) -> bool {
        matches!(self, Seg::Line(..))
    }
    pub fn bbox(&self) -> Rect {
        let mut r = Rect::empty();
        r.add(self.start());
        r.add(self.end());
        if let Seg::Arc { c, r: rad, a0, sw, .. } = *self {
            for k in 0..4 {
                let ang = k as f64 * PI / 2.0;
                if let Some(_) = arc_frac(a0, sw, ang) {
                    r.add(pt(c.x + rad * ang.cos(), c.y + rad * ang.sin()));
                }
            }
        }
        r
    }
    /// Unit tangent direction at parameter t.
    pub fn tangent(&self, t: f64) -> Pt {
        match *self {
            Seg::Line(a, b) => (b - a).norm(),
            Seg::Arc { a0, sw, .. } => {
                let a = a0 + sw * t;
                let d = pt(-a.sin(), a.cos());
                if sw >= 0.0 { d } else { d * -1.0 }
            }
        }
    }
    /// Parameter of the closest point on the segment to p.
    pub fn closest_t(&self, p: Pt) -> f64 {
        match *self {
            Seg::Line(a, b) => {
                let d = b - a;
                let l2 = d.dot(d);
                if l2 < 1e-24 { 0.0 } else { ((p - a).dot(d) / l2).clamp(0.0, 1.0) }
            }
            Seg::Arc { c, a0, sw, .. } => {
                let ang = (p.y - c.y).atan2(p.x - c.x);
                if let Some(t) = arc_frac(a0, sw, ang) {
                    return t.clamp(0.0, 1.0);
                }
                let d0 = p.dist(self.start());
                let d1 = p.dist(self.end());
                if d0 <= d1 { 0.0 } else { 1.0 }
            }
        }
    }
    pub fn dist(&self, p: Pt) -> f64 {
        match *self {
            Seg::Arc { c, r, .. } => {
                let t = self.closest_t(p);
                let q = self.at(t);
                // use radial distance when the projection falls inside the arc
                if t > 0.0 && t < 1.0 { (p.dist(c) - r).abs() } else { p.dist(q) }
            }
            _ => p.dist(self.at(self.closest_t(p))),
        }
    }
}

/// Fraction along arc (a0, sw) of the angle `ang`, if within the arc (with tolerance).
pub fn arc_frac(a0: f64, sw: f64, ang: f64) -> Option<f64> {
    let tol = 1e-9;
    let mut d = if sw >= 0.0 { ang - a0 } else { a0 - ang };
    d = d.rem_euclid(TAU);
    let s = sw.abs();
    if d > TAU - tol {
        d -= TAU;
    }
    if d >= -tol && d <= s + tol { Some((d / s).clamp(0.0, 1.0)) } else { None }
}

pub fn loop_segs(l: &Loop) -> Vec<Seg> {
    let n = l.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = l[i];
        let b = l[(i + 1) % n];
        out.push(bulge_to_seg(a.p(), b.p(), a.b));
    }
    out
}

/// Segments of an open polyline.
pub fn poly_segs(l: &[V]) -> Vec<Seg> {
    let mut out = Vec::new();
    for i in 0..l.len().saturating_sub(1) {
        out.push(bulge_to_seg(l[i].p(), l[i + 1].p(), l[i].b));
    }
    out
}

/// Intersection parameters (ta, tb) of two segments (transversal crossings and tangent touches).
pub fn intersect(a: &Seg, b: &Seg) -> Vec<(f64, f64)> {
    match (a, b) {
        (Seg::Line(p, q), Seg::Line(r, s)) => {
            let d1 = *q - *p;
            let d2 = *s - *r;
            let den = d1.cross(d2);
            let l1 = d1.len();
            let l2 = d2.len();
            if den.abs() < 1e-12 * (l1 * l2).max(1e-30) {
                return vec![];
            }
            let w = *r - *p;
            let t = w.cross(d2) / den;
            let u = w.cross(d1) / den;
            let e1 = TOL / l1.max(1e-12);
            let e2 = TOL / l2.max(1e-12);
            if t >= -e1 && t <= 1.0 + e1 && u >= -e2 && u <= 1.0 + e2 { vec![(t.clamp(0.0, 1.0), u.clamp(0.0, 1.0))] } else { vec![] }
        }
        (Seg::Line(p, q), Seg::Arc { c, r, a0, sw, .. }) => line_arc(*p, *q, *c, *r, *a0, *sw).into_iter().collect(),
        (Seg::Arc { c, r, a0, sw, .. }, Seg::Line(p, q)) => line_arc(*p, *q, *c, *r, *a0, *sw).into_iter().map(|(t, u)| (u, t)).collect(),
        (Seg::Arc { c: c1, r: r1, a0: a01, sw: sw1, .. }, Seg::Arc { c: c2, r: r2, a0: a02, sw: sw2, .. }) => {
            let d = c1.dist(*c2);
            if d < 1e-12 {
                return vec![];
            }
            if d > r1 + r2 + TOL || d < (r1 - r2).abs() - TOL {
                return vec![];
            }
            let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
            let h2 = r1 * r1 - x * x;
            let h = if h2 < 0.0 { 0.0 } else { h2.sqrt() };
            let dir = (*c2 - *c1) * (1.0 / d);
            let base = *c1 + dir * x;
            let mut out = vec![];
            let cands = if h < 1e-9 { vec![base] } else { vec![base + dir.perp() * h, base - dir.perp() * h] };
            for p in cands {
                let ang1 = (p.y - c1.y).atan2(p.x - c1.x);
                let ang2 = (p.y - c2.y).atan2(p.x - c2.x);
                if let (Some(t), Some(u)) = (arc_frac(*a01, *sw1, ang1), arc_frac(*a02, *sw2, ang2)) {
                    out.push((t, u));
                }
            }
            out
        }
    }
}

fn line_arc(p: Pt, q: Pt, c: Pt, r: f64, a0: f64, sw: f64) -> Vec<(f64, f64)> {
    let d = q - p;
    let f = p - c;
    let a = d.dot(d);
    if a < 1e-24 {
        return vec![];
    }
    let b = 2.0 * f.dot(d);
    let cc = f.dot(f) - r * r;
    let disc = b * b - 4.0 * a * cc;
    let l = a.sqrt();
    let e = TOL / l;
    let mut out = vec![];
    if disc < -1e-9 * a.max(1.0) {
        return out;
    }
    let sq = if disc < 0.0 { 0.0 } else { disc.sqrt() };
    let roots = if sq < 1e-9 { vec![-b / (2.0 * a)] } else { vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)] };
    for t in roots {
        if t < -e || t > 1.0 + e {
            continue;
        }
        let tt = t.clamp(0.0, 1.0);
        let pt_ = p + d * tt;
        let ang = (pt_.y - c.y).atan2(pt_.x - c.x);
        if let Some(u) = arc_frac(a0, sw, ang) {
            out.push((tt, u));
        }
    }
    out
}

/// For two collinear overlapping line segments, the parameter interval of `b` within `a`.
pub fn collinear_overlap(a: &Seg, b: &Seg, tol: f64) -> Option<(f64, f64)> {
    if let (Seg::Line(p, q), Seg::Line(r, s)) = (a, b) {
        let d = *q - *p;
        let l = d.len();
        if l < 1e-12 {
            return None;
        }
        let n = d.perp() * (1.0 / l);
        if (*r - *p).dot(n).abs() > tol || (*s - *p).dot(n).abs() > tol {
            return None;
        }
        let u = d * (1.0 / l);
        let t0 = (*r - *p).dot(u) / l;
        let t1 = (*s - *p).dot(u) / l;
        let (lo, hi) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
        let lo = lo.max(0.0);
        let hi = hi.min(1.0);
        if hi - lo > tol / l { Some((lo, hi)) } else { None }
    } else {
        None
    }
}

/// Flatten an arc into points (excluding start, including end) with chord error <= tol.
pub fn flatten_seg_pts(s: &Seg, tol: f64, out: &mut Vec<Pt>) {
    match *s {
        Seg::Line(_, b) => out.push(b),
        Seg::Arc { c, r, a0, sw, p1, .. } => {
            let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { PI / 2.0 };
            let step = step.clamp(0.02, PI / 4.0);
            let n = ((sw.abs() / step).ceil() as usize).max(1);
            for i in 1..n {
                let a = a0 + sw * (i as f64 / n as f64);
                out.push(pt(c.x + r * a.cos(), c.y + r * a.sin()));
            }
            out.push(p1);
        }
    }
}

/// Flatten a closed loop into a polygon (vertex list, not repeated).
pub fn flatten_loop(l: &Loop, tol: f64) -> Vec<Pt> {
    let mut out = Vec::with_capacity(l.len() + 8);
    for s in loop_segs(l) {
        out.push(s.start());
        if let Seg::Arc { .. } = s {
            let mut tmp = vec![];
            flatten_seg_pts(&s, tol, &mut tmp);
            tmp.pop();
            out.extend(tmp);
        }
    }
    out
}

pub fn flatten_poly(l: &[V], tol: f64) -> Vec<Pt> {
    let mut out = Vec::new();
    if l.is_empty() {
        return out;
    }
    out.push(l[0].p());
    for s in poly_segs(l) {
        flatten_seg_pts(&s, tol, &mut out);
    }
    out
}

pub fn poly_area(p: &[Pt]) -> f64 {
    let n = p.len();
    let mut a = 0.0;
    for i in 0..n {
        let q = p[(i + 1) % n];
        a += p[i].cross(q);
    }
    a * 0.5
}

/// Signed area of a bulge loop (CCW positive), exact for arcs.
pub fn loop_area(l: &Loop) -> f64 {
    let mut a = 0.0;
    let n = l.len();
    for i in 0..n {
        let p = l[i].p();
        let q = l[(i + 1) % n].p();
        a += p.cross(q) * 0.5;
        if l[i].b.abs() > 1e-12 {
            let s = bulge_to_seg(p, q, l[i].b);
            if let Seg::Arc { r, sw, .. } = s {
                a += 0.5 * r * r * (sw - sw.sin());
            }
        }
    }
    a
}

pub fn reverse_loop(l: &Loop) -> Loop {
    let n = l.len();
    (0..n)
        .map(|k| {
            let src = n - 1 - k;
            let prev = (src + n - 1) % n;
            V { x: l[src].x, y: l[src].y, b: -l[prev].b }
        })
        .collect()
}

pub fn make_ccw(l: &Loop) -> Loop {
    if loop_area(l) < 0.0 { reverse_loop(l) } else { l.clone() }
}
pub fn make_cw(l: &Loop) -> Loop {
    if loop_area(l) > 0.0 { reverse_loop(l) } else { l.clone() }
}

pub fn loop_bbox(l: &Loop) -> Rect {
    let mut r = Rect::empty();
    for s in loop_segs(l) {
        r.union(&s.bbox());
    }
    r
}

pub fn region_bbox(r: &Region) -> Rect {
    loop_bbox(&r.outer)
}

pub fn poly_bbox(p: &[Pt]) -> Rect {
    let mut r = Rect::empty();
    for q in p {
        r.add(*q);
    }
    r
}

/// Even-odd crossing test against a flattened polygon.
pub fn poly_contains(p: &[Pt], q: Pt) -> bool {
    let n = p.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (p[i], p[j]);
        if (a.y > q.y) != (b.y > q.y) {
            let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if q.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    Inside,
    Outside,
    Boundary,
}

pub fn loop_dist(l: &Loop, p: Pt) -> f64 {
    loop_segs(l).iter().map(|s| s.dist(p)).fold(f64::INFINITY, f64::min)
}

pub fn loop_locate(l: &Loop, p: Pt, tol: f64) -> Loc {
    if loop_dist(l, p) <= tol {
        return Loc::Boundary;
    }
    let poly = flatten_loop(l, 0.0005);
    if poly_contains(&poly, p) { Loc::Inside } else { Loc::Outside }
}

pub fn region_locate(r: &Region, p: Pt, tol: f64) -> Loc {
    match loop_locate(&r.outer, p, tol) {
        Loc::Outside => Loc::Outside,
        Loc::Boundary => Loc::Boundary,
        Loc::Inside => {
            for h in &r.holes {
                match loop_locate(h, p, tol) {
                    Loc::Inside => return Loc::Outside,
                    Loc::Boundary => return Loc::Boundary,
                    Loc::Outside => {}
                }
            }
            Loc::Inside
        }
    }
}

/// Centroid of a polygon (shoelace); falls back to the vertex average.
pub fn poly_centroid(p: &[Pt]) -> Pt {
    let n = p.len();
    let a = poly_area(p);
    if a.abs() < 1e-12 {
        let mut s = pt(0.0, 0.0);
        for q in p {
            s = s + *q;
        }
        return s * (1.0 / n.max(1) as f64);
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..n {
        let q = p[(i + 1) % n];
        let w = p[i].cross(q);
        cx += (p[i].x + q.x) * w;
        cy += (p[i].y + q.y) * w;
    }
    pt(cx / (6.0 * a), cy / (6.0 * a))
}

/// Remove consecutive duplicate vertices (keeping the bulge of the first of a run).
pub fn dedupe_loop(l: &Loop) -> Loop {
    let mut out: Loop = Vec::with_capacity(l.len());
    for p in l {
        if let Some(last) = out.last() {
            if last.p().near(p.p(), 1e-9) {
                continue;
            }
        }
        out.push(*p);
    }
    while out.len() > 1 && out[0].p().near(out[out.len() - 1].p(), 1e-9) {
        out.pop();
    }
    out
}

/// Segment-vs-axis-aligned-rect clipping: returns parameter intervals of `s` inside the rect.
pub fn clip_seg_rect(s: &Seg, r: &Rect) -> Vec<(f64, f64)> {
    let edges = [
        Seg::Line(pt(r.x0, r.y0), pt(r.x1, r.y0)),
        Seg::Line(pt(r.x1, r.y0), pt(r.x1, r.y1)),
        Seg::Line(pt(r.x1, r.y1), pt(r.x0, r.y1)),
        Seg::Line(pt(r.x0, r.y1), pt(r.x0, r.y0)),
    ];
    let mut ts = vec![0.0, 1.0];
    for e in &edges {
        for (t, _) in intersect(s, e) {
            ts.push(t);
        }
    }
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out: Vec<(f64, f64)> = vec![];
    for w in ts.windows(2) {
        if w[1] - w[0] < 1e-12 {
            continue;
        }
        let m = s.at((w[0] + w[1]) * 0.5);
        if r.contains(m, 1e-9) {
            if let Some(last) = out.last_mut() {
                if (last.1 - w[0]).abs() < 1e-12 {
                    last.1 = w[1];
                    continue;
                }
            }
            out.push((w[0], w[1]));
        }
    }
    out
}

pub fn rot_about(p: Pt, c: Pt, ang: f64) -> Pt {
    c + (p - c).rot(ang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulge_semicircle() {
        let s = bulge_to_seg(pt(-1.0, 0.0), pt(1.0, 0.0), 1.0);
        if let Seg::Arc { c, r, sw, .. } = s {
            assert!(c.near(pt(0.0, 0.0), 1e-9));
            assert!((r - 1.0).abs() < 1e-9);
            assert!((sw - PI).abs() < 1e-9);
            // CCW from (-1,0) passes through (0,-1)
            assert!(s.at(0.5).near(pt(0.0, -1.0), 1e-9));
        } else {
            panic!()
        }
    }

    #[test]
    fn circle_area() {
        let l = vec![vb(-1.0, 0.0, 1.0), vb(1.0, 0.0, 1.0)];
        assert!((loop_area(&l) - PI).abs() < 1e-9);
        let rl = reverse_loop(&l);
        assert!((loop_area(&rl) + PI).abs() < 1e-9);
    }

    #[test]
    fn line_circle() {
        let a = bulge_to_seg(pt(-1.0, 0.0), pt(1.0, 0.0), 1.0);
        let l = Seg::Line(pt(0.0, -2.0), pt(0.0, 2.0));
        let x = intersect(&a, &l);
        assert_eq!(x.len(), 1);
        assert!(a.at(x[0].0).near(pt(0.0, -1.0), 1e-9));
    }

    #[test]
    fn locate() {
        let sq = Rect::new(0.0, 0.0, 2.0, 2.0).loop_();
        assert_eq!(loop_locate(&sq, pt(1.0, 1.0), 1e-7), Loc::Inside);
        assert_eq!(loop_locate(&sq, pt(3.0, 1.0), 1e-7), Loc::Outside);
        assert_eq!(loop_locate(&sq, pt(2.0, 1.0), 1e-7), Loc::Boundary);
    }
}
