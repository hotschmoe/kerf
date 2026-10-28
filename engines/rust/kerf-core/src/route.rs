//! Note routing (SPEC 6.3, 16, 18): column placement, leader de-crossing, landing-point nudging and
//! leader-hit detection.
//!
//! Pure geometry; no document types. The caller (annot) supplies the notes' text-block sizes,
//! candidate landing points (index 0 is the SPEC 6.3 label point), the dimension-text / label boxes
//! that leaders must stay away from, and the dimension line segments (soft obstacles).
//!
//! Algorithm (deterministic; no randomness, no hash order):
//!  1. Baseline = SPEC 6.3 (sort by landing y, stack top-down) + the SPEC 16 adjacent-swap de-crossing.
//!     A clean baseline is returned untouched.
//!  2. If any leader crosses or comes within one text height of another leader, a note box, a dimension
//!     text or a label, each column is re-solved exactly by dynamic programming: for notes in their
//!     column order, pick a landing candidate and a vertical position (a grid of half-pitch steps around
//!     the landing y) minimizing the sum of per-leader costs (obstacle hits, dimension-line crossings,
//!     steepness, length, distance from the preferred landing) plus the leader/leader cost of adjacent
//!     notes, subject to the text blocks not overlapping. Both columns are solved in turn (each sees the
//!     other's leaders as fixed). If hits remain, discrete changes are tried one at a time and re-solved:
//!     a note moves to the other column (`notes_side` "both"), swaps with a neighbour, or is re-inserted
//!     at another position of its column; the first change that lowers (hits, cost) is kept.
//!  3. Whatever hits remain are returned with concrete fix proposals (`at` / `place`).

use crate::geom::{self, Box, V2};
use crate::math;
use crate::sort;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::Cell;

#[derive(Clone, Copy, Debug)]
pub struct Geo {
    pub h: f64,
    pub pitch: f64,
    pub gap: f64,
    pub shoulder: f64,
    pub pad: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug)]
pub struct NoteIn<'a> {
    /// Text block size (model units).
    pub w: f64,
    pub hgt: f64,
    /// Candidate landing points; `cands[0]` is the preferred (SPEC 6.3) one.
    pub cands: &'a [V2],
    /// False when the author gave `at` (the landing is exact and never nudged).
    pub movable: bool,
    /// Designer override of the text position (top-left of the block).
    pub place: Option<V2>,
    /// Per-note column hint (`column` on the note): the note never leaves that column.
    pub column: Option<Side>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObstKind {
    Dim,
    Label,
}

#[derive(Clone, Copy, Debug)]
pub struct Obst {
    pub kind: ObstKind,
    pub poly: [V2; 4],
}

/// Layout effort counter: one unit per leader/leader, leader/box or leader/obstacle penalty evaluation. The search
/// stops improving once `used >= limit` and the layout falls back to what it has; the work is shared by every pass and
/// every trial of one view build, so no document can cost more than a bounded amount of time.
#[derive(Debug)]
pub struct Work {
    used: Cell<u64>,
    pub limit: u64,
}

/// 40 million units is about 1.5 s of release speed.
pub const DEFAULT_WORK_LIMIT: u64 = 40_000_000;

impl Work {
    pub fn new(limit: u64) -> Work {
        Work { used: Cell::new(0), limit }
    }
    pub fn used(&self) -> u64 {
        self.used.get()
    }
    pub fn spent(&self) -> bool {
        self.used.get() >= self.limit
    }
    fn add(&self, n: u64) {
        self.used.set(self.used.get().saturating_add(n));
    }
}

impl Default for Work {
    fn default() -> Work {
        Work::new(DEFAULT_WORK_LIMIT)
    }
}

/// `Layout.hits` keeps every hit, but concrete fix proposals are only computed for the first `MAX_FIX_HITS` of them.
pub const MAX_FIX_HITS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct Params<'a> {
    pub geo: Geo,
    pub crop: Box,
    pub xl: f64,
    pub xr: f64,
    pub gutter: f64,
    pub side: Side,
    /// Re-routes inside the caller's repair loop: one search attempt with a small budget.
    pub light: bool,
    /// Shared effort counter; `None` = a fresh default budget for this call.
    pub work: Option<&'a Work>,
}

impl<'a> Params<'a> {
    pub fn new(geo: Geo, crop: Box, xl: f64, xr: f64, gutter: f64, side: Side) -> Params<'a> {
        Params { geo, crop, xl, xr, gutter, side, light: false, work: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Leader,
    Note,
    Dim,
    Label,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// The note whose leader is involved.
    pub note: usize,
    pub kind: HitKind,
    /// Note index (leader, note) or obstacle index (dim, label).
    pub other: usize,
    /// Smallest distance between the leader and the other item (0 = they cross), model units.
    pub dist: f64,
    pub fix_at: Option<V2>,
    pub fix_place: Option<V2>,
    /// Same, for the other note when kind is `Leader`.
    pub other_fix_at: Option<V2>,
    pub other_fix_place: Option<V2>,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub top: Vec<f64>,
    pub x: Vec<f64>,
    pub left: Vec<bool>,
    pub landing: Vec<V2>,
    pub leaders: Vec<[V2; 3]>,
    pub hits: Vec<Hit>,
    pub searched: bool,
    /// The work budget ran out: the notes are placed by the basic rule (plus whatever the search had found) and hits may remain.
    pub budget_exhausted: bool,
}

const W_CROSS: f64 = 1000.0;
const W_NEAR_BASE: f64 = 100.0;
const W_NEAR: f64 = 200.0;
const W_SOFT_CROSS: f64 = 25.0;

// ---- geometry helpers --------------------------------------------------------------------------------

fn cross3(o: V2, a: V2, b: V2) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Plain floating point proper-or-touching intersection test (leaders are drawn, not constructed:
/// no exact arithmetic needed, and this is the hot loop of the search).
fn segs_cross(a0: V2, a1: V2, b0: V2, b1: V2) -> bool {
    let d1 = cross3(b0, b1, a0);
    let d2 = cross3(b0, b1, a1);
    let d3 = cross3(a0, a1, b0);
    let d4 = cross3(a0, a1, b1);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    let eps = 1e-12;
    (d1.abs() <= eps && on_seg(b0, b1, a0))
        || (d2.abs() <= eps && on_seg(b0, b1, a1))
        || (d3.abs() <= eps && on_seg(a0, a1, b0))
        || (d4.abs() <= eps && on_seg(a0, a1, b1))
}

fn on_seg(a: V2, b: V2, p: V2) -> bool {
    p.x >= a.x.min(b.x) - 1e-12
        && p.x <= a.x.max(b.x) + 1e-12
        && p.y >= a.y.min(b.y) - 1e-12
        && p.y <= a.y.max(b.y) + 1e-12
}

fn pt_seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let l2 = abx * abx + aby * aby;
    let mut t = 0.0;
    if l2 > 0.0 {
        t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / l2).clamp(0.0, 1.0);
    }
    let dx = p.x - (a.x + t * abx);
    let dy = p.y - (a.y + t * aby);
    math::sqrt(dx * dx + dy * dy)
}

pub fn seg_seg_dist(a0: V2, a1: V2, b0: V2, b1: V2) -> f64 {
    if segs_cross(a0, a1, b0, b1) {
        return 0.0;
    }
    pt_seg_dist(a0, b0, b1).min(pt_seg_dist(a1, b0, b1)).min(pt_seg_dist(b0, a0, a1).min(pt_seg_dist(b1, a0, a1)))
}

pub fn poly_poly_dist(p: &[V2; 3], q: &[V2; 3]) -> f64 {
    let mut d = f64::INFINITY;
    for i in 0..2 {
        for j in 0..2 {
            d = d.min(seg_seg_dist(p[i], p[i + 1], q[j], q[j + 1]));
        }
    }
    d
}

pub fn poly_box_dist(p: &[V2; 3], b: &[V2; 4]) -> f64 {
    if geom::point_in_loop_eo(p[0], b) || geom::point_in_loop_eo(p[1], b) || geom::point_in_loop_eo(p[2], b) {
        return 0.0;
    }
    let mut d = f64::INFINITY;
    for i in 0..2 {
        for j in 0..4 {
            d = d.min(seg_seg_dist(p[i], p[i + 1], b[j], b[(j + 1) % 4]));
        }
    }
    d
}

pub fn leader_of(left_side: bool, x: f64, top: f64, w: f64, hgt: f64, landing: V2, g: &Geo) -> [V2; 3] {
    let ymid = top - hgt * 0.5;
    let edge_x = if left_side { x + w + g.pad } else { x - g.pad };
    let dir = if left_side { 1.0 } else { -1.0 };
    [V2::init(edge_x, ymid), V2::init(edge_x + dir * g.shoulder, ymid), landing]
}

fn box_poly(x: f64, top: f64, w: f64, hgt: f64) -> [V2; 4] {
    [V2::init(x, top - hgt), V2::init(x + w, top - hgt), V2::init(x + w, top), V2::init(x, top)]
}

// ---- layout context ----------------------------------------------------------------------------------

/// `hard` counts the hits between notes (leader/leader, leader/text block); `hits` all of them, dimension text and
/// labels included. Dimension text and labels can be moved by the caller afterwards, so the search ranks `hard` first.
#[derive(Clone, Copy)]
struct Cost {
    total: f64,
    hits: usize,
    hard: usize,
}

#[derive(Clone, Copy)]
struct BB {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl BB {
    fn of_leader(l: &[V2; 3]) -> BB {
        BB {
            x0: l[0].x.min(l[1].x.min(l[2].x)),
            y0: l[0].y.min(l[1].y.min(l[2].y)),
            x1: l[0].x.max(l[1].x.max(l[2].x)),
            y1: l[0].y.max(l[1].y.max(l[2].y)),
        }
    }

    fn of_box(b: &[V2; 4]) -> BB {
        BB {
            x0: b[0].x.min(b[1].x).min(b[2].x.min(b[3].x)),
            y0: b[0].y.min(b[1].y).min(b[2].y.min(b[3].y)),
            x1: b[0].x.max(b[1].x).max(b[2].x.max(b[3].x)),
            y1: b[0].y.max(b[1].y).max(b[2].y.max(b[3].y)),
        }
    }

    fn apart(&self, b: &BB, d: f64) -> bool {
        self.x0 > b.x1 + d || b.x0 > self.x1 + d || self.y0 > b.y1 + d || b.y0 > self.y1 + d
    }
}

#[derive(Clone, Copy)]
struct Slot {
    ci: usize,
    y: f64,
    lead: [V2; 3],
    bb: BB,
    u: f64,
}

const ZERO_BB: BB = BB { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
const ZERO_SLOT: Slot = Slot { ci: 0, y: 0.0, lead: [V2 { x: 0.0, y: 0.0 }; 3], bb: ZERO_BB, u: 0.0 };

const DP_CANDS: usize = 6;
const DP_J: i32 = 12;
const DP_SLOTS: usize = DP_CANDS * (2 * DP_J as usize + 1);

struct Snap {
    left: Vec<bool>,
    ci: Vec<usize>,
    key: Vec<f64>,
    ov: Vec<f64>,
    hits: usize,
    hard: usize,
    total: f64,
}

struct Ctx<'a> {
    p: Params<'a>,
    notes: &'a [NoteIn<'a>],
    obst: &'a [Obst],
    obst_bb: Vec<BB>,
    soft: &'a [[V2; 2]],
    left: Vec<bool>,
    ci: Vec<usize>,
    key: Vec<f64>,
    /// Explicit text-block top chosen by the column solver (NaN = use the stacking rule).
    ov: Vec<f64>,
    top: Vec<f64>,
    x: Vec<f64>,
    leaders: Vec<[V2; 3]>,
    boxes: Vec<[V2; 4]>,
    slots: Vec<Slot>,
    dpc: Vec<f64>,
    back: Vec<i32>,
    cnt: Vec<usize>,
    /// Remaining column solves (bounds the work on dense views).
    budget: usize,
    work: &'a Work,
}

impl Ctx<'_> {
    fn pref(&self, i: usize) -> V2 {
        self.notes[i].cands.first().copied().unwrap_or_default()
    }

    fn landing(&self, i: usize) -> V2 {
        let c = self.notes[i].cands;
        c.get(self.ci[i]).or(c.first()).copied().unwrap_or_default()
    }

    fn fixed(&self, i: usize) -> bool {
        self.notes[i].place.is_some()
    }

    fn sort_order(&self, col_left: bool) -> Vec<usize> {
        let mut ord: Vec<usize> =
            (0..self.notes.len()).filter(|&i| !self.fixed(i) && self.left[i] == col_left).collect();
        let key = &self.key;
        sort::stable(&mut ord, |&x, &y| if key[x] != key[y] { key[x] < key[y] } else { x < y });
        ord
    }

    fn shift_column(&mut self, ord: &[usize]) {
        let (Some(&first), Some(&last)) = (ord.first(), ord.last()) else { return };
        let bottom = self.top[last] - self.notes[last].hgt;
        if bottom < self.p.crop.y0 {
            let d = self.p.crop.y0 - bottom;
            for &i in ord {
                self.top[i] += d;
            }
        }
        let t = self.top[first];
        if t > self.p.crop.y1 {
            let d = t - self.p.crop.y1;
            for &i in ord {
                self.top[i] -= d;
            }
        }
    }

    /// SPEC 6.3 step 4: initial y = landing y, resolve overlaps top-down, then shift the column into the crop.
    fn stack_top_down(&mut self, ord: &[usize]) {
        let mut prev_bottom = f64::INFINITY;
        for &i in ord {
            let mut t = self.landing(i).y + self.notes[i].hgt * 0.5;
            if t > prev_bottom - self.p.geo.gap {
                t = prev_bottom - self.p.geo.gap;
            }
            self.top[i] = t;
            prev_bottom = t - self.notes[i].hgt;
        }
        self.shift_column(ord);
    }

    fn layout(&mut self) {
        for col_left in [false, true] {
            let ord = self.sort_order(col_left);
            for &i in &ord {
                self.x[i] =
                    if col_left { self.p.xl - self.p.gutter - self.notes[i].w } else { self.p.xr + self.p.gutter };
            }
            self.stack_top_down(&ord);
            for &i in &ord {
                if !self.ov[i].is_nan() {
                    self.top[i] = self.ov[i];
                }
            }
        }
        for i in 0..self.notes.len() {
            let nt = self.notes[i];
            if let Some(pl) = nt.place {
                self.x[i] = pl.x;
                self.top[i] = pl.y;
                self.left[i] = pl.x + nt.w * 0.5 < self.landing(i).x;
            }
            self.leaders[i] =
                leader_of(self.left[i], self.x[i], self.top[i], nt.w, nt.hgt, self.landing(i), &self.p.geo);
            self.boxes[i] = box_poly(self.x[i], self.top[i], nt.w, nt.hgt);
        }
    }

    /// Penalty for a leader/box or leader/leader distance (None = clear).
    fn near(&self, d: f64) -> Option<f64> {
        if d <= 1e-9 {
            return Some(W_CROSS);
        }
        if d < self.p.geo.h {
            return Some(W_NEAR_BASE + W_NEAR * (1.0 - d / self.p.geo.h));
        }
        None
    }

    fn pair_pen(&self, a: &[V2; 3], ab: &BB, b: &[V2; 3], bb: &BB) -> f64 {
        self.work.add(1);
        if ab.apart(bb, self.p.geo.h) {
            return 0.0;
        }
        self.near(poly_poly_dist(a, b)).unwrap_or(0.0)
    }

    fn box_pen(&self, a: &[V2; 3], ab: &BB, b: &[V2; 4], bb: &BB) -> f64 {
        self.work.add(1);
        if ab.apart(bb, self.p.geo.h) {
            return 0.0;
        }
        self.near(poly_box_dist(a, b)).unwrap_or(0.0)
    }

    fn soft_pen(&self, l: &[V2; 3], lb: &BB) -> f64 {
        let h = self.p.geo.h;
        let mut c = 0.0;
        for s in self.soft {
            let sb =
                BB { x0: s[0].x.min(s[1].x), y0: s[0].y.min(s[1].y), x1: s[0].x.max(s[1].x), y1: s[0].y.max(s[1].y) };
            if lb.apart(&sb, h * 0.5) {
                continue;
            }
            let mut dmin = seg_seg_dist(l[0], l[1], s[0], s[1]);
            dmin = dmin.min(seg_seg_dist(l[1], l[2], s[0], s[1]));
            if dmin <= 1e-9 {
                c += W_SOFT_CROSS;
            } else if dmin < h * 0.5 {
                c += 5.0;
            }
        }
        c
    }

    /// Leader shape: prefer short, shallow leaders and landings near the preferred point.
    fn shape_pen(&self, l: &[V2; 3], land: V2, pref: V2) -> f64 {
        let h = self.p.geo.h;
        let dy = (l[2].y - l[1].y).abs();
        let dx = (l[2].x - l[1].x).abs();
        let mut c = 0.4 * dy / h + 0.02 * l[1].dist(l[2]) / h + 0.5 * land.dist(pref) / h;
        if dy > 1.5 * dx {
            c += 3.0;
        }
        c
    }

    fn cost(&self) -> Cost {
        let mut c = 0.0;
        let mut hits = 0;
        let mut hard = 0;
        let n = self.notes.len();
        for i in 0..n {
            let li = &self.leaders[i];
            let lb = BB::of_leader(li);
            for j in i + 1..n {
                let w = self.pair_pen(li, &lb, &self.leaders[j], &BB::of_leader(&self.leaders[j]));
                if w > 0.0 {
                    c += w;
                    hits += 1;
                    hard += 1;
                }
            }
            for j in 0..n {
                if j == i {
                    continue;
                }
                let w = self.box_pen(li, &lb, &self.boxes[j], &BB::of_box(&self.boxes[j]));
                if w > 0.0 {
                    c += w;
                    hits += 1;
                    hard += 1;
                }
            }
            for (k, o) in self.obst.iter().enumerate() {
                let w = self.box_pen(li, &lb, &o.poly, &self.obst_bb[k]);
                if w > 0.0 {
                    c += w;
                    hits += 1;
                }
            }
            c += self.soft_pen(li, &lb);
            c += self.shape_pen(li, self.landing(i), self.pref(i));
        }
        Cost { total: c, hits, hard }
    }

    fn eval(&mut self) -> Cost {
        self.layout();
        self.cost()
    }

    /// Hits involving note i's leader only.
    fn hits_of(&self, i: usize) -> usize {
        let mut k = 0;
        let li = &self.leaders[i];
        let lb = BB::of_leader(li);
        for j in 0..self.notes.len() {
            if j == i {
                continue;
            }
            if self.pair_pen(li, &lb, &self.leaders[j], &BB::of_leader(&self.leaders[j])) > 0.0 {
                k += 1;
            }
            if self.box_pen(li, &lb, &self.boxes[j], &BB::of_box(&self.boxes[j])) > 0.0 {
                k += 1;
            }
        }
        for (m, o) in self.obst.iter().enumerate() {
            if self.box_pen(li, &lb, &o.poly, &self.obst_bb[m]) > 0.0 {
                k += 1;
            }
        }
        k
    }

    fn snapshot(&mut self) -> Snap {
        let cst = self.eval();
        Snap {
            left: self.left.clone(),
            ci: self.ci.clone(),
            key: self.key.clone(),
            ov: self.ov.clone(),
            hits: cst.hits,
            hard: cst.hard,
            total: cst.total,
        }
    }

    fn restore(&mut self, s: &Snap) {
        self.left.clone_from(&s.left);
        self.ci.clone_from(&s.ci);
        self.key.clone_from(&s.key);
        self.ov.clone_from(&s.ov);
    }

    // ---- exact column solver -------------------------------------------------------------------------

    /// Re-solve one column by dynamic programming (see the module doc). Returns false (state untouched) when
    /// no feasible assignment exists.
    fn solve_column(&mut self, col_left: bool) -> bool {
        let g = self.p.geo;
        let ord = self.sort_order(col_left);
        let n = ord.len();
        if n == 0 {
            return true;
        }
        if self.budget == 0 || self.work.spent() {
            return false;
        }
        self.budget -= 1;
        let total_h: f64 = ord.iter().map(|&i| self.notes[i].hgt).sum::<f64>() + g.gap * (n - 1) as f64;
        let span = total_h.max(self.p.crop.y1 - self.p.crop.y0);
        let mid = (self.p.crop.y0 + self.p.crop.y1) * 0.5;
        let lo = mid - span * 0.5 - 2.0 * g.pitch;
        let hi = mid + span * 0.5 + 2.0 * g.pitch;
        let step = g.pitch * 0.5;
        let nn = self.notes.len();
        let mut adjacent = vec![false; nn];
        for (k, &i) in ord.iter().enumerate() {
            let nt = self.notes[i];
            let base = k * DP_SLOTS;
            let mut cnt = 0;
            let ncand = if nt.movable { DP_CANDS.min(nt.cands.len()) } else { 1 };
            let xi = if col_left { self.p.xl - self.p.gutter - nt.w } else { self.p.xr + self.p.gutter };
            adjacent.fill(false);
            if k > 0 {
                adjacent[ord[k - 1]] = true;
            }
            if k + 1 < n {
                adjacent[ord[k + 1]] = true;
            }
            let pref = self.pref(i);
            for ci in 0..ncand {
                let Some(&land) = nt.cands.get(ci) else { break };
                for jj in 0..=DP_J {
                    for sgn in [1i32, -1] {
                        if jj == 0 && sgn == -1 {
                            continue;
                        }
                        let y = land.y + (jj * sgn) as f64 * step;
                        let tp = y + nt.hgt * 0.5;
                        if tp > hi || y - nt.hgt * 0.5 < lo {
                            continue;
                        }
                        let lead = leader_of(col_left, xi, tp, nt.w, nt.hgt, land, &g);
                        let lb = BB::of_leader(&lead);
                        let mut u = self.shape_pen(&lead, land, pref) + self.soft_pen(&lead, &lb);
                        for (m, o) in self.obst.iter().enumerate() {
                            u += self.box_pen(&lead, &lb, &o.poly, &self.obst_bb[m]);
                        }
                        #[allow(clippy::needless_range_loop)]
                        for j in 0..nn {
                            if j == i {
                                continue;
                            }
                            let in_col = !self.fixed(j) && self.left[j] == col_left;
                            if in_col {
                                if adjacent[j] {
                                    continue;
                                }
                                u += self.pair_pen(&lead, &lb, &self.leaders[j], &BB::of_leader(&self.leaders[j]));
                                continue;
                            }
                            u += self.pair_pen(&lead, &lb, &self.leaders[j], &BB::of_leader(&self.leaders[j]));
                            u += self.box_pen(&lead, &lb, &self.boxes[j], &BB::of_box(&self.boxes[j]));
                        }
                        self.slots[base + cnt] = Slot { ci, y, lead, bb: lb, u };
                        cnt += 1;
                    }
                }
            }
            self.cnt[k] = cnt;
            if cnt == 0 {
                return false;
            }
        }
        let inf = f64::INFINITY;
        for s in 0..self.cnt[0] {
            self.dpc[s] = self.slots[s].u;
            self.back[s] = -1;
        }
        for k in 1..n {
            let hk = self.notes[ord[k]].hgt;
            let hp = self.notes[ord[k - 1]].hgt;
            let base = k * DP_SLOTS;
            let pbase = (k - 1) * DP_SLOTS;
            for s in 0..self.cnt[k] {
                let sl = self.slots[base + s];
                let mut best = inf;
                let mut bi: i32 = -1;
                for q in 0..self.cnt[k - 1] {
                    let pc = self.dpc[pbase + q];
                    if pc >= best {
                        continue;
                    }
                    let ps = self.slots[pbase + q];
                    if ps.y - hp * 0.5 - g.gap < sl.y + hk * 0.5 - 1e-9 {
                        continue;
                    }
                    let v = pc + self.pair_pen(&ps.lead, &ps.bb, &sl.lead, &sl.bb);
                    if v < best {
                        best = v;
                        bi = q as i32;
                    }
                }
                self.dpc[base + s] = if bi < 0 { inf } else { best + sl.u };
                self.back[base + s] = bi;
            }
        }
        let mut best = inf;
        let mut bs = 0usize;
        let lbase = (n - 1) * DP_SLOTS;
        for s in 0..self.cnt[n - 1] {
            if self.dpc[lbase + s] < best {
                best = self.dpc[lbase + s];
                bs = s;
            }
        }
        if best == inf {
            return false;
        }
        let mut s = bs as i32;
        for k in (0..n).rev() {
            let idx = k * DP_SLOTS + s as usize;
            let sl = self.slots[idx];
            let i = ord[k];
            self.ci[i] = sl.ci;
            self.ov[i] = sl.y + self.notes[i].hgt * 0.5;
            s = self.back[idx];
        }
        true
    }

    fn solve_all(&mut self) {
        for _ in 0..2 {
            self.solve_column(false);
            self.layout();
            self.solve_column(true);
            self.layout();
        }
    }
}

fn better(a: Cost, b: Cost) -> bool {
    better_mode(a, b, true)
}

/// Lexicographic: hits between notes (if `hard_first`), all hits, cost.
fn better_mode(a: Cost, b: Cost, hard_first: bool) -> bool {
    if hard_first && a.hard != b.hard {
        return a.hard < b.hard;
    }
    a.hits < b.hits || (a.hits == b.hits && a.total < b.total - 1e-9)
}

/// Hit-driven improvement: DP re-solve, then single discrete changes (column flip, neighbour swap,
/// re-insertion) re-solved one at a time; the first change that lowers (hits, cost) is kept.
fn improve(c: &mut Ctx) {
    let n = c.notes.len();
    c.solve_all();
    let mut cur = c.eval();
    let mut rounds = 0;
    while cur.hits > 0 && rounds < 6 && c.budget > 0 && !c.work.spent() {
        rounds += 1;
        let mut changed = false;
        let mut i = 0;
        while i < n && !changed && c.budget > 0 && !c.work.spent() {
            let ci = i;
            i += 1;
            if c.fixed(ci) || c.hits_of(ci) == 0 {
                continue;
            }
            let start = c.snapshot();
            if c.p.side == Side::Both && c.notes[ci].column.is_none() {
                c.left[ci] = !c.left[ci];
                c.key[ci] = -c.landing(ci).y;
                c.solve_all();
                let r = c.eval();
                if better(r, cur) {
                    cur = r;
                    changed = true;
                    continue;
                }
                c.restore(&start);
                c.eval();
            }
            let mut cnt = 0;
            let mut pos = 0;
            for j in 0..n {
                if c.fixed(j) || c.left[j] != c.left[ci] {
                    continue;
                }
                if j != ci && (c.key[j] < c.key[ci] || (c.key[j] == c.key[ci] && j < ci)) {
                    pos += 1;
                }
                cnt += 1;
            }
            let mut v = 0;
            while v < cnt && !changed && c.budget > 0 && !c.work.spent() {
                let cv = v;
                v += 1;
                if cv == pos {
                    continue;
                }
                let ord = c.sort_order(c.left[ci]);
                rekey_at(&mut c.key, &ord, ci, cv);
                c.solve_all();
                let r = c.eval();
                if better(r, cur) {
                    cur = r;
                    changed = true;
                    break;
                }
                c.restore(&start);
                c.eval();
            }
        }
        if !changed {
            break;
        }
    }
}

/// Renumber the keys of a column (`ord`: its notes in key order, `i` among them) to 0, 1, ... with note `i` moved to position
/// `v` and the others keeping their order. Every note of the column gets a fresh key, however many there are.
fn rekey_at(key: &mut [f64], ord: &[usize], i: usize, v: usize) {
    let mut k = 0usize;
    for &o in ord {
        if o == i {
            continue;
        }
        if k == v {
            key[i] = k as f64;
            k += 1;
        }
        key[o] = k as f64;
        k += 1;
    }
    if k == v {
        key[i] = k as f64;
    }
}

/// SPEC 16: swap adjacent column notes whose leaders cross (bounded), then renumber the keys by rank so later
/// changes are position changes.
fn decross(c: &mut Ctx) {
    let n = c.notes.len();
    for _ in 0..n * 4 + 8 {
        let mut swapped = false;
        for col_left in [false, true] {
            let ord = c.sort_order(col_left);
            for w in ord.windows(2) {
                if poly_poly_dist(&c.leaders[w[0]], &c.leaders[w[1]]) <= 1e-9 {
                    c.key.swap(w[0], w[1]);
                    swapped = true;
                    break;
                }
            }
            if swapped {
                break;
            }
        }
        if !swapped {
            break;
        }
        c.layout();
    }
    for col_left in [false, true] {
        let ord = c.sort_order(col_left);
        for (k, &i) in ord.iter().enumerate() {
            c.key[i] = k as f64;
        }
    }
}

// ---- entry point -------------------------------------------------------------------------------------

pub fn route(p: Params, notes: &[NoteIn], obst: &[Obst], soft: &[[V2; 2]]) -> Layout {
    let n = notes.len();
    let local_work = Work::default();
    let work = p.work.unwrap_or(&local_work);
    let mut c = Ctx {
        p,
        notes,
        obst,
        obst_bb: obst.iter().map(|o| BB::of_box(&o.poly)).collect(),
        soft,
        left: vec![false; n],
        ci: vec![0; n],
        key: vec![0.0; n],
        ov: vec![f64::NAN; n],
        top: vec![0.0; n],
        x: vec![0.0; n],
        leaders: vec![[V2::default(); 3]; n],
        boxes: vec![[V2::default(); 4]; n],
        slots: vec![ZERO_SLOT; n * DP_SLOTS],
        dpc: vec![0.0; n * DP_SLOTS],
        back: vec![0; n * DP_SLOTS],
        cnt: vec![0; n],
        budget: 60,
        work,
    };
    for (i, nt) in notes.iter().enumerate() {
        let l = nt.cands.first().copied().unwrap_or_default();
        c.left[i] = match nt.column {
            Some(cs) => cs == Side::Left,
            None => match p.side {
                Side::Left => true,
                Side::Right => false,
                Side::Both => (l.x - p.crop.x0).abs() < (p.crop.x1 - l.x).abs(),
            },
        };
        c.key[i] = -l.y;
    }
    c.layout();
    decross(&mut c);
    let mut searched = false;
    if c.eval().hits > 0 {
        searched = true;
        let mut best = c.snapshot();
        c.budget = if p.light { 20 } else { 60 };
        c.eval();
        improve(&mut c);
        let r = c.snapshot();
        let rc = Cost { total: r.total, hits: r.hits, hard: r.hard };
        let bc = Cost { total: best.total, hits: best.hits, hard: best.hard };
        if better_mode(rc, bc, true) {
            best = r;
        }
        c.restore(&best);
        c.eval();
    }

    let mut hits: Vec<Hit> = Vec::new();
    let mk = |note, kind, other, dist| Hit {
        note,
        kind,
        other,
        dist,
        fix_at: None,
        fix_place: None,
        other_fix_at: None,
        other_fix_place: None,
    };
    for i in 0..n {
        for j in i + 1..n {
            let d = poly_poly_dist(&c.leaders[i], &c.leaders[j]);
            if c.near(d).is_some() {
                hits.push(mk(i, HitKind::Leader, j, d));
            }
        }
        for j in 0..n {
            if j == i {
                continue;
            }
            let d = poly_box_dist(&c.leaders[i], &c.boxes[j]);
            if c.near(d).is_some() {
                hits.push(mk(i, HitKind::Note, j, d));
            }
        }
        for (j, o) in obst.iter().enumerate() {
            let d = poly_box_dist(&c.leaders[i], &o.poly);
            if c.near(d).is_some() {
                hits.push(mk(i, if o.kind == ObstKind::Dim { HitKind::Dim } else { HitKind::Label }, j, d));
            }
        }
    }
    for (hi, ht) in hits.iter_mut().enumerate() {
        if hi >= MAX_FIX_HITS {
            break;
        }
        let (fa, fp) = propose_fix(&mut c, ht.note);
        ht.fix_at = fa;
        ht.fix_place = fp;
        if ht.kind == HitKind::Leader {
            let (oa, op) = propose_fix(&mut c, ht.other);
            ht.other_fix_at = oa;
            ht.other_fix_place = op;
        }
    }
    c.layout();
    let landing: Vec<V2> = (0..n).map(|i| c.landing(i)).collect();
    Layout {
        top: c.top,
        x: c.x,
        left: c.left,
        landing,
        leaders: c.leaders,
        hits,
        searched,
        budget_exhausted: work.spent(),
    }
}

/// A concrete way to clear note i's hits: an alternative landing point (`at`) or a new text position
/// (`place`), found by trying the alternatives with every other note where it is now.
fn propose_fix(c: &mut Ctx, i: usize) -> (Option<V2>, Option<V2>) {
    let mut fix_at = None;
    let mut fix_place = None;
    c.layout();
    if c.hits_of(i) == 0 {
        return (fix_at, fix_place);
    }
    let nt = c.notes[i];
    if nt.movable && !c.fixed(i) {
        let save = c.ci[i];
        for v in 0..nt.cands.len() {
            if v == save {
                continue;
            }
            c.ci[i] = v;
            let old = c.leaders[i];
            c.leaders[i] = leader_of(c.left[i], c.x[i], c.top[i], nt.w, nt.hgt, c.landing(i), &c.p.geo);
            let k = c.hits_of(i);
            c.leaders[i] = old;
            if k == 0 {
                fix_at = Some(nt.cands[v]);
                break;
            }
        }
        c.ci[i] = save;
    }
    let step = c.p.geo.pitch * 0.5;
    let top0 = c.top[i];
    let old_l = c.leaders[i];
    let old_b = c.boxes[i];
    let mut k = 1;
    while k <= 24 && fix_place.is_none() {
        for sg in [1.0f64, -1.0] {
            let t = top0 + sg * k as f64 * step;
            c.leaders[i] = leader_of(c.left[i], c.x[i], t, nt.w, nt.hgt, c.landing(i), &c.p.geo);
            c.boxes[i] = box_poly(c.x[i], t, nt.w, nt.hgt);
            let mut ok = c.hits_of(i) == 0;
            if ok {
                for j in 0..c.notes.len() {
                    if j == i {
                        continue;
                    }
                    let bj = c.boxes[j];
                    if c.x[j] < c.x[i] + nt.w
                        && c.x[i] < c.x[j] + c.notes[j].w
                        && bj[0].y < t + c.p.geo.gap
                        && t - nt.hgt - c.p.geo.gap < bj[2].y
                    {
                        ok = false;
                    }
                }
            }
            if ok {
                fix_place = Some(V2::init(c.x[i], t));
                break;
            }
        }
        k += 1;
    }
    c.leaders[i] = old_l;
    c.boxes[i] = old_b;
    (fix_at, fix_place)
}
