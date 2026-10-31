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
    /// The offset as written; `off0` differs when the dimension is lined up with its chain (`chain_dims`).
    pub authored: f64,
    /// The chain the dimension belongs to: the index of its first member (itself when it chains with nothing).
    pub chain: usize,
    /// Whether the low / high end of the dimension line (left / bottom first) continues into another dimension of
    /// its chain, so text that does not fit goes past the chain's free ends first.
    pub shared: [bool; 2],
}

impl DimSpec<'_> {
    pub fn sgn(&self) -> f64 {
        if self.off0 >= 0.0 { 1.0 } else { -1.0 }
    }

    /// The coordinate the dimension measures (x for h, y for v).
    fn measured(&self, p: V2) -> f64 {
        if self.dir == DimDir::V { p.y } else { p.x }
    }

    /// The coordinate its offset is taken across (y for h, x for v).
    fn across(&self, p: V2) -> f64 {
        if self.dir == DimDir::V { p.x } else { p.y }
    }

    /// The end points, lower measured coordinate first.
    fn ends(&self) -> [V2; 2] {
        if self.measured(self.from) <= self.measured(self.to) { [self.from, self.to] } else { [self.to, self.from] }
    }

    /// Where the offset counts from: the outer of the two points on the side of the offset.
    fn base(&self) -> f64 {
        let (a, b) = (self.across(self.from), self.across(self.to));
        if self.off0 >= 0.0 { a.max(b) } else { a.min(b) }
    }

    /// The across coordinate of the dimension line at the authored offset.
    fn line_at(&self) -> f64 {
        self.base() + self.off0
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

/// The outside-text place only chain members get: centred on the own span, a text row out (see `dim_build`).
const CENTRED_OUT: usize = 6;

/// Two chain ends meet when their measured coordinates agree to this (model inches).
const CHAIN_EPS: f64 = 1.0 / 64.0;

/// SPEC 28.3 chain dimensions: h (or v) dimensions on the same side whose spans follow each other end to end (they
/// share an end point, or an end coordinate with dimension lines within `near` of each other) form one chain, and every
/// member is drawn on the chain's outermost authored line. A chain never holds two overlapping spans.
pub fn chain_dims(specs: &mut [DimSpec], near: f64) {
    let n = specs.len();
    let mut root: Vec<usize> = (0..n).collect();
    let mut shared: Vec<[bool; 2]> = alloc::vec![[false; 2]; n];
    let find = |root: &[usize], mut i: usize| {
        while root[i] != i {
            i = root[i];
        }
        i
    };
    let overlaps = |a: &DimSpec, b: &DimSpec| {
        let (ea, eb) = (a.ends(), b.ends());
        let lo = a.measured(ea[0]).max(b.measured(eb[0]));
        let hi = a.measured(ea[1]).min(b.measured(eb[1]));
        hi - lo > CHAIN_EPS
    };
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&specs[i], &specs[j]);
            if a.dir != b.dir || a.dir == DimDir::Aligned || a.sgn() != b.sgn() || a.span() < CHAIN_EPS || b.span() < CHAIN_EPS
            {
                continue;
            }
            let (ea, eb) = (a.ends(), b.ends());
            let meet = [(ea[1], eb[0]), (eb[1], ea[0])].map(|(p, q)| {
                (a.measured(p) - a.measured(q)).abs() <= CHAIN_EPS
                    && (p.dist(q) <= CHAIN_EPS || (a.line_at() - b.line_at()).abs() <= near)
            });
            if !meet[0] && !meet[1] {
                continue;
            }
            let (ri, rj) = (find(&root, i), find(&root, j));
            if ri != rj {
                let clash = (0..n).any(|x| {
                    find(&root, x) == ri && (0..n).any(|y| find(&root, y) == rj && overlaps(&specs[x], &specs[y]))
                });
                if clash {
                    continue;
                }
                let (lo, hi) = (ri.min(rj), ri.max(rj));
                root[hi] = lo;
            }
            if meet[0] {
                shared[i][1] = true;
                shared[j][0] = true;
            }
            if meet[1] {
                shared[j][1] = true;
                shared[i][0] = true;
            }
        }
    }
    for i in 0..n {
        let r = find(&root, i);
        specs[i].chain = r;
        specs[i].shared = shared[i];
    }
    for r in 0..n {
        let members: Vec<usize> = (0..n).filter(|&i| specs[i].chain == r).collect();
        if members.len() < 2 {
            continue;
        }
        let lines = members.iter().map(|&i| specs[i].line_at());
        let line = if specs[r].sgn() > 0.0 { lines.fold(f64::NEG_INFINITY, f64::max) } else { lines.fold(f64::INFINITY, f64::min) };
        for &i in &members {
            let base = specs[i].base();
            specs[i].off0 = line - base;
        }
    }
}

/// Build one dimension. `offset` is the signed dimension-line offset (SPEC 16). When the text does not fit
/// between the extension lines it goes outside (SPEC 20), never smaller and never dropped, and never on its own line
/// (SPEC 28.3). `variant` picks where: 0/1 beside the dimension line extended past the far / near end, 2-5 raised out
/// of the axis (outward / inward of the object) at the far / near end, joined to the line by a short leader, 6 (chain
/// members) centred on its own span a text row out on the side away from the object, joined by a leader. `slide` moves text that fits
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
    let tight = lb.sub(la).len() < 2.0 * tick;
    if tight {
        let (a0, b0) = (la.sub(u.scale(2.0 * tick)), lb.add(u.scale(2.0 * tick)));
        out.push(path_item(env, Pen::Dim, d.id, &[a0, b0], false));
        sh.add(a0, b0);
        let r = (0.02 * sc).min(lb.sub(la).len() * 0.3).max(0.005 * sc);
        for p in [la, lb] {
            out.push(Item::Fill(crate::drawing::FillItem {
                layer: alloc::string::String::from(env.style.layer_name(LayerKey::Dims)),
                src: alloc::string::String::from(d.id),
                loops: alloc::vec![crate::model::circle_loop(p.x, p.y, r)],
            }));
        }
    } else {
        out.push(path_item(env, Pen::Dim, d.id, &[la, lb], false));
        sh.add(la, lb);
        let tdir = u.add(nrm).norm();
        for p in [la, lb] {
            let t0 = p.sub(tdir.scale(tick * 0.5));
            let t1 = p.add(tdir.scale(tick * 0.5));
            out.push(path_item(env, Pen::Profile, d.id, &[t0, t1], false));
            sh.add(t0, t1);
        }
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
        let at_far = matches!(variant, 0 | 2 | 4);
        let e = if at_far { lb } else { la };
        let s = if at_far { 1.0 } else { -1.0 };
        if variant == CENTRED_OUT {
            valign = VAlign::Middle;
            let out_n = d.axis().scale(d.sgn());
            let m = la.mid(lb);
            let q = m.add(out_n.scale(th + tgap));
            out.push(path_item(env, Pen::Dim, d.id, &[m, q], false));
            sh.add(m, q);
            m.add(out_n.scale(1.5 * th + 2.0 * tgap))
        } else if variant < 2 {
            let q = e.add(u.scale(s * (2.0 * tick + tw + tgap)));
            out.push(path_item(env, Pen::Dim, d.id, &[e, q], false));
            sh.add(e, q);
            e.add(u.scale(s * (2.0 * tick + tw * 0.5))).add(tn.scale(tgap))
        } else {
            valign = VAlign::Middle;
            let outward = nrm.scale(d.sgn());
            let lift = if variant < 4 { th + tgap } else { -(th + tgap) };
            let q = e.add(u.scale(s * 2.0 * tick)).add(outward.scale(lift));
            out.push(path_item(env, Pen::Dim, d.id, &[e, q], false));
            sh.add(e, q);
            q.add(u.scale(s * (tgap + tw * 0.5)))
        }
    };
    let ti = text_item(env, LayerKey::Dims, Pen::Dim, d.id, &label, center.x, center.y, th, ang, Align::Center, valign);
    if let Item::Text(t) = &ti {
        sh.text = text_poly(env.font, t, 0.015 * sc);
    }
    out.push(ti);
    sh
}

/// A label: one text item per line (a newline in `text` breaks it), the block centered vertically on `at` and each line
/// aligned on it.
pub fn label_items(env: &mut Env, id: &str, text: &str, at: V2, align: Align, out: &mut Vec<Item>) {
    let t = upper_if(env, text).into_owned();
    let h = env.style.label_height_in * env.s;
    let pitch = h * env.style.line_spacing;
    let lines: Vec<&str> = t.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let top = at.y + pitch * (lines.len() - 1) as f64 / 2.0;
    for (i, line) in lines.iter().enumerate() {
        let y = top - pitch * i as f64;
        out.push(text_item(env, LayerKey::Notes, Pen::Anno, id, line, at.x, y, h, 0.0, align, VAlign::Middle));
    }
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
/// inch (outside text first tries the other places before the dimension line moves). A chain (SPEC 28.3) is placed
/// and moved as one, by its overall span; text that does not fit goes past a free end of the chain before a shared
/// one. `pushes` is the extra outward distance chosen by the layout repair (a chain takes its largest), `slides` the
/// text slides. A pinned dimension (or a chain with a pinned member) stays at its offset. Fills the effective offsets
/// `eff`, the items, and per dimension `blockers`: what it collides with at its authored offset (pushes included),
/// named for the diagnostics (SPEC 25.2).
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
    let tol = 0.5 * step;
    let mut units: Vec<Vec<usize>> = Vec::new();
    for (i, d) in specs.iter().enumerate() {
        match units.iter_mut().find(|u| specs[u[0]].chain == d.chain) {
            Some(u) => u.push(i),
            None => units.push(alloc::vec![i]),
        }
    }
    let span_of = |u: &[usize]| {
        if let [one] = u {
            return specs[*one].span();
        }
        let (lo, hi) = u.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &i| {
            let e = specs[i].ends();
            (lo.min(specs[i].measured(e[0])), hi.max(specs[i].measured(e[1])))
        });
        hi - lo
    };
    sort::stable(&mut units, |x, y| {
        let sx = span_of(x);
        let sy = span_of(y);
        if (sx - sy).abs() > 1e-9 { sx < sy } else { x[0] < y[0] }
    });
    let mut placed: Vec<(usize, DimShape)> = Vec::new();
    for unit in units {
        let push = unit.iter().map(|&i| pushes.get(i).copied().unwrap_or(0.0)).fold(0.0, f64::max);
        let kmax = if unit.iter().any(|&i| specs[i].pin) { 0 } else { 12u32 };
        let mut best: Vec<(usize, DimPlaced)> = Vec::new();
        let mut best_c = usize::MAX;
        for k in 0..=kmax {
            let mut picks: Vec<(usize, DimPlaced)> = Vec::new();
            let mut total = 0usize;
            for &i in &unit {
                let d = &specs[i];
                let off = d.off0 + d.sgn() * (push + f64::from(k) * step);
                let slide = slides.get(i).copied().unwrap_or(0.0);
                let mut kbest: Option<DimPlaced> = None;
                let mut kbest_rank = (usize::MAX, usize::MAX, usize::MAX);
                let chained = d.shared[0] || d.shared[1];
                for v in (0..DIM_VARIANTS).filter(|&v| v != CENTRED_OUT || chained) {
                    let mut its = Vec::new();
                    let sh = dim_build(env, d, off, v, slide, &mut its);
                    let sib: usize = picks.iter().map(|(_, p)| dim_conflicts(&sh, &p.shape, tol)).sum();
                    let at_shared = v != CENTRED_OUT && d.shared[usize::from(matches!(v, 0 | 2 | 4))];
                    let beside_sibling = !sh.fits && at_shared && v < 2;
                    let c = conflicts_of(&sh, &placed, labels, tol) + sib + usize::from(beside_sibling);
                    let pen = match (sh.fits, v) {
                        (true, _) => 0,
                        (false, CENTRED_OUT) => 1,
                        _ if at_shared => 2,
                        _ => 0,
                    };
                    let bh = if sh.fits { 0 } else { base_hits(base_segs, &sh.text) };
                    let rank = (c, pen, bh);
                    let fits = sh.fits;
                    if rank < kbest_rank {
                        kbest_rank = rank;
                        kbest = Some(DimPlaced { offset: off, shape: sh, items: its });
                    }
                    if fits {
                        break;
                    }
                }
                let Some(kb) = kbest else { continue };
                if k == 0
                    && kbest_rank.0 > 0
                    && let Some(why) = blockers.get_mut(i)
                {
                    let sibs = picks.iter().map(|(j, p)| (*j, p.shape));
                    for (j, o) in placed.iter().copied().chain(sibs) {
                        if dim_conflicts(&kb.shape, &o, tol) > 0 {
                            why.push(format!("dimension '{}'", specs[j].id));
                        }
                    }
                    for (lid, lp) in labels {
                        if conflicts_of(&kb.shape, &[], &[(lid, *lp)], tol) > 0 {
                            why.push(format!("label '{lid}'"));
                        }
                    }
                }
                total += kbest_rank.0;
                picks.push((i, kb));
            }
            if total < best_c {
                best_c = total;
                best = picks;
            }
            if total == 0 {
                break;
            }
        }
        for (i, pick) in best {
            if let Some(e) = eff.get_mut(i) {
                *e = pick.offset;
            }
            if let Some(it) = items.get_mut(i) {
                *it = pick.items;
            }
            placed.push((i, pick.shape));
        }
    }
}
