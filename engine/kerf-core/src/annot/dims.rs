//! Dimensions (SPEC 6.5): the dimension line, extension lines, terminators and text of one dimension, and the stacking of dimensions that would collide.

use super::text::{DEG_PER_RAD, RAD_PER_DEG, ascii_fold, path_item, text_item, text_poly, upper_if};
use super::{DIM_VARIANTS, DimDir, Env};
use crate::drawing::{Align, Item, VAlign};
use crate::geom::{self, Box, V2};
use crate::math;
use crate::pen::{LayerKey, Pen};
use crate::route;
use crate::sort;
use crate::units;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// A parsed dimension. Its drawn offset is `off0` (authored) pushed outward by `push` (layout repair) and by
/// the automatic stacking (SPEC 20).
#[derive(Clone, Copy, Debug)]
pub struct DimSpec<'a> {
    pub k: usize,
    pub id: &'a str,
    pub from: V2,
    pub to: V2,
    pub dir: DimDir,
    pub off0: f64,
    pub text: Option<&'a str>,
    /// `pin`: drawn at `off0` exactly, never stacked or pushed (overlaps then warn W_LABEL_OVERLAP).
    pub pin: bool,
}

impl DimSpec<'_> {
    pub fn sgn(&self) -> f64 {
        if self.off0 >= 0.0 { 1.0 } else { -1.0 }
    }

    /// Length measured by the dimension (model units).
    pub fn span(&self) -> f64 {
        match self.dir {
            DimDir::V => (self.to.y - self.from.y).abs(),
            DimDir::Aligned => self.from.dist(self.to),
            DimDir::H => (self.to.x - self.from.x).abs(),
        }
    }

    /// Unit vector along the dimension line (from the lower / left end), the direction its text slides in.
    pub fn along(&self) -> V2 {
        match self.dir {
            DimDir::V => V2::init(0.0, 1.0),
            DimDir::Aligned => self.to.sub(self.from).norm(),
            DimDir::H => V2::init(1.0, 0.0),
        }
    }

    /// Unit vector along which the line moves when |offset| grows (before the sign of the offset).
    pub fn axis(&self) -> V2 {
        match self.dir {
            DimDir::V => V2::init(1.0, 0.0),
            DimDir::Aligned => self.to.sub(self.from).norm().perp(),
            DimDir::H => V2::init(0.0, 1.0),
        }
    }
}

/// The drawn pieces of one dimension, for conflict tests (SPEC 20 stacking and outside text).
#[derive(Clone, Copy, Debug)]
pub struct DimShape {
    pub segs: [[V2; 2]; 6],
    pub n: usize,
    /// The dimension line proper (between the extension lines).
    pub line: [V2; 2],
    /// The text box (padded).
    pub text: [V2; 4],
    pub fits: bool,
}

impl DimShape {
    fn new() -> DimShape {
        let z = V2::default();
        DimShape { segs: [[z; 2]; 6], n: 0, line: [z; 2], text: [z; 4], fits: true }
    }

    fn add(&mut self, a: V2, b: V2) {
        if let Some(s) = self.segs.get_mut(self.n) {
            *s = [a, b];
            self.n += 1;
        }
    }

    pub fn used(&self) -> &[[V2; 2]] {
        &self.segs[..self.n]
    }
}

/// Build one dimension. `offset` is the signed dimension-line offset (SPEC 16). When the text does not fit
/// between the extension lines it goes outside (SPEC 20), never smaller and never dropped, joined to the
/// dimension line by a short leader. `variant` picks where: 0/1 on the axis beyond the end of the line, 2-5
/// raised out of the axis (outward / inward of the object) at the far / near end. `slide` moves text that fits
/// along the line from its middle (the layout repair slides it off a note leader, SPEC 25.2).
pub fn dim_build(env: &mut Env, d: &DimSpec, offset: f64, variant: usize, slide: f64, out: &mut Vec<Item>) -> DimShape {
    let st = env.style;
    let sc = env.s;
    let gap = st.ext_gap_in * sc;
    let over = st.ext_over_in * sc;
    let tick = st.tick_len_in * sc;
    let th = st.text_height_in * sc;
    let tgap = st.dim_text_gap_in * sc;
    let from = d.from;
    let to = d.to;
    let (pa, pb, la, lb, u, nrm);
    match d.dir {
        DimDir::V => {
            let x = if offset >= 0.0 { from.x.max(to.x) + offset } else { from.x.min(to.x) + offset };
            pa = V2::init(x, from.y);
            pb = V2::init(x, to.y);
            (la, lb) = if from.y <= to.y { (pa, pb) } else { (pb, pa) };
            u = V2::init(0.0, 1.0);
            nrm = V2::init(-1.0, 0.0);
        }
        DimDir::Aligned => {
            let dd = to.sub(from).norm();
            let n = dd.perp();
            pa = from.add(n.scale(offset));
            pb = to.add(n.scale(offset));
            la = pa;
            lb = pb;
            u = dd;
            nrm = n;
        }
        DimDir::H => {
            let y = if offset >= 0.0 { from.y.max(to.y) + offset } else { from.y.min(to.y) + offset };
            pa = V2::init(from.x, y);
            pb = V2::init(to.x, y);
            (la, lb) = if from.x <= to.x { (pa, pb) } else { (pb, pa) };
            u = V2::init(1.0, 0.0);
            nrm = V2::init(0.0, 1.0);
        }
    }
    let mut sh = DimShape::new();
    for (p0, p1) in [(from, pa), (to, pb)] {
        let dv = p1.sub(p0);
        if dv.len() < 1e-9 {
            continue;
        }
        let dn = dv.norm();
        let e0 = p0.add(dn.scale(gap));
        let e1 = p1.add(dn.scale(over));
        out.push(path_item(env, Pen::Dim, d.id, &[e0, e1], false));
        sh.add(e0, e1);
    }
    let dist = d.span();
    let label = d.text.map_or_else(|| units::fmt_ft_in(dist), alloc::string::String::from);
    let label_f = ascii_fold(&label);
    let tw = env.font.width(&label_f, th);
    let fits = tw + 2.0 * tgap <= lb.sub(la).len() - tick;
    sh.fits = fits;
    sh.line = [la, lb];
    out.push(path_item(env, Pen::Dim, d.id, &[la, lb], false));
    sh.add(la, lb);
    let tdir = u.add(nrm).norm();
    for p in [la, lb] {
        let t0 = p.sub(tdir.scale(tick * 0.5));
        let t1 = p.add(tdir.scale(tick * 0.5));
        out.push(path_item(env, Pen::Profile, d.id, &[t0, t1], false));
        sh.add(t0, t1);
    }
    let mut ang = math::atan2(u.y, u.x) * DEG_PER_RAD;
    if ang > 90.0 + 1e-9 || ang <= -90.0 + 1e-9 {
        ang += 180.0;
    }
    if d.dir == DimDir::V {
        ang = 90.0;
    }
    let tn = V2::init(-math::sin(ang * RAD_PER_DEG), math::cos(ang * RAD_PER_DEG));
    let mut valign = VAlign::Baseline;
    let center = if fits {
        la.mid(lb).add(u.scale(slide)).add(tn.scale(tgap))
    } else {
        valign = VAlign::Middle;
        let at_far = matches!(variant, 0 | 2 | 4);
        let e = if at_far { lb } else { la };
        let s = if at_far { 1.0 } else { -1.0 };
        let outward = nrm.scale(d.sgn());
        let lift = match variant {
            2 | 3 => th + tgap,
            4 | 5 => -(th + tgap),
            _ => 0.0,
        };
        let q = e.add(u.scale(s * 2.0 * tick)).add(outward.scale(lift));
        out.push(path_item(env, Pen::Dim, d.id, &[e, q], false));
        sh.add(e, q);
        q.add(u.scale(s * (tgap + tw * 0.5)))
    };
    let ti = text_item(env, LayerKey::Dims, Pen::Dim, d.id, &label, center.x, center.y, th, ang, Align::Center, valign);
    if let Item::Text(t) = &ti {
        sh.text = text_poly(env.font, t, 0.015 * sc);
    }
    out.push(ti);
    sh
}

pub fn label_items(env: &mut Env, id: &str, text: &str, at: V2, out: &mut Vec<Item>) {
    let t = upper_if(env, text).into_owned();
    let h = env.style.label_height_in * env.s;
    out.push(text_item(env, LayerKey::Notes, Pen::Anno, id, &t, at.x, at.y, h, 0.0, Align::Center, VAlign::Middle));
}

pub fn poly_hits_seg(poly: &[V2; 4], a: V2, b: V2) -> bool {
    if geom::point_in_loop_eo(a, poly) || geom::point_in_loop_eo(b, poly) {
        return true;
    }
    (0..4).any(|i| route::seg_seg_dist(a, b, poly[i], poly[(i + 1) % 4]) <= 1e-9)
}

pub fn polys_overlap(p: &[V2; 4], q: &[V2; 4]) -> bool {
    (0..4).any(|i| poly_hits_seg(q, p[i], p[(i + 1) % 4])) || (0..4).any(|i| geom::point_in_loop_eo(q[i], p))
}

/// Parallel dimension lines that run on top of each other (within `tol`, with a shared stretch).
pub fn lines_collide(a: [V2; 2], b: [V2; 2], tol: f64) -> bool {
    let da = a[1].sub(a[0]);
    let db = b[1].sub(b[0]);
    let la = da.len();
    let lb = db.len();
    if la < 1e-9 || lb < 1e-9 {
        return false;
    }
    let ua = da.scale(1.0 / la);
    if ua.cross(db.scale(1.0 / lb)).abs() > 0.02 {
        return false;
    }
    let t0 = b[0].sub(a[0]).dot(ua);
    let t1 = b[1].sub(a[0]).dot(ua);
    let ov = la.min(t0.max(t1)) - 0.0f64.max(t0.min(t1));
    if ov <= 1e-6 {
        return false;
    }
    ua.cross(b[0].sub(a[0])).abs() < tol
}

pub fn dim_conflicts(x: &DimShape, y: &DimShape, tol: f64) -> usize {
    let mut c = 0;
    if lines_collide(x.line, y.line, tol) {
        c += 1;
    }
    if polys_overlap(&x.text, &y.text) {
        c += 1;
    }
    if y.used().iter().any(|sg| poly_hits_seg(&x.text, sg[0], sg[1])) {
        c += 1;
    }
    if x.used().iter().any(|sg| poly_hits_seg(&y.text, sg[0], sg[1])) {
        c += 1;
    }
    c
}

pub fn base_hits(segs: &[[V2; 2]], poly: &[V2; 4]) -> usize {
    let mut bb = Box::default();
    for p in poly {
        bb.add_point(p.x, p.y);
    }
    segs.iter()
        .filter(|sg| {
            if sg[0].x.max(sg[1].x) < bb.x0
                || sg[0].x.min(sg[1].x) > bb.x1
                || sg[0].y.max(sg[1].y) < bb.y0
                || sg[0].y.min(sg[1].y) > bb.y1
            {
                return false;
            }
            poly_hits_seg(poly, sg[0], sg[1])
        })
        .count()
}

struct DimPlaced {
    offset: f64,
    shape: DimShape,
    items: Vec<Item>,
}

/// What a dimension's text or lines collide with: the other dimensions already placed and the labels.
fn conflicts_of(sh: &DimShape, placed: &[(usize, DimShape)], labels: &[(&str, [V2; 4])], tol: f64) -> usize {
    let mut c: usize = placed.iter().map(|(_, o)| dim_conflicts(sh, o, tol)).sum();
    for (_, lp) in labels {
        if polys_overlap(&sh.text, lp) {
            c += 1;
        }
        if sh.used().iter().any(|sg| poly_hits_seg(lp, sg[0], sg[1])) {
            c += 1;
        }
    }
    c
}

/// SPEC 20 dimension stacking: dims are placed shortest first; a dimension whose line would overlap another
/// one, or whose text would overprint another dimension's text or lines, moves out in steps of 0.25 paper
/// inch (outside text first tries the other places before the dimension line moves). `pushes` is the extra
/// outward distance chosen by the layout repair, `slides` the text slides. A pinned dimension stays at its offset. Fills the effective
/// offsets `eff`, the items, and per dimension `blockers`: what it collides with at its authored offset (pushes
/// included), named for the diagnostics (SPEC 25.2).
#[allow(clippy::too_many_arguments)]
pub fn stack_dims(
    env: &mut Env,
    specs: &[DimSpec],
    pushes: &[f64],
    slides: &[f64],
    base_segs: &[[V2; 2]],
    labels: &[(&str, [V2; 4])],
    items: &mut [Vec<Item>],
    eff: &mut [f64],
    blockers: &mut [Vec<String>],
) {
    let step = 0.25 * env.s;
    let mut order: Vec<usize> = (0..specs.len()).collect();
    sort::stable(&mut order, |&x, &y| {
        let sx = specs[x].span();
        let sy = specs[y].span();
        if (sx - sy).abs() > 1e-9 { sx < sy } else { x < y }
    });
    let mut placed: Vec<(usize, DimShape)> = Vec::new();
    for i in order {
        let d = &specs[i];
        let start = d.off0 + d.sgn() * pushes.get(i).copied().unwrap_or(0.0);
        let slide = slides.get(i).copied().unwrap_or(0.0);
        let mut best: Option<DimPlaced> = None;
        let mut best_c = usize::MAX;
        let kmax = if d.pin { 0 } else { 12u32 };
        for k in 0..=kmax {
            let off = start + d.sgn() * f64::from(k) * step;
            let mut kbest: Option<DimPlaced> = None;
            let mut kbest_hits = usize::MAX;
            let mut kbest_c = usize::MAX;
            for v in 0..DIM_VARIANTS {
                let mut its = Vec::new();
                let sh = dim_build(env, d, off, v, slide, &mut its);
                let c = conflicts_of(&sh, &placed, labels, 0.5 * step);
                let bh = if sh.fits { 0 } else { base_hits(base_segs, &sh.text) };
                let fits = sh.fits;
                if c < kbest_c || (c == kbest_c && bh < kbest_hits) {
                    kbest_c = c;
                    kbest_hits = bh;
                    kbest = Some(DimPlaced { offset: off, shape: sh, items: its });
                }
                if fits {
                    break;
                }
            }
            if k == 0
                && kbest_c > 0
                && let (Some(kb), Some(why)) = (&kbest, blockers.get_mut(i))
            {
                for (j, o) in &placed {
                    if dim_conflicts(&kb.shape, o, 0.5 * step) > 0 {
                        why.push(format!("dimension '{}'", specs[*j].id));
                    }
                }
                for (lid, lp) in labels {
                    if conflicts_of(&kb.shape, &[], &[(lid, *lp)], 0.5 * step) > 0 {
                        why.push(format!("label '{lid}'"));
                    }
                }
            }
            if kbest_c < best_c {
                best_c = kbest_c;
                best = kbest;
            }
            if kbest_c == 0 {
                break;
            }
        }
        let Some(pick) = best else { continue };
        if let Some(e) = eff.get_mut(i) {
            *e = pick.offset;
        }
        if let Some(it) = items.get_mut(i) {
            *it = pick.items;
        }
        placed.push((i, pick.shape));
    }
}
