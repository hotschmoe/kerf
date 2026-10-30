//! Polygon booleans (intersection, union, difference) on flattened polygons.
//!
//! Method: arrangement + classification. Every boundary edge of A and B is split at all mutual
//! intersections (robust orientation predicates decide crossings, touches and collinear overlaps;
//! cut points are computed once per pair so both edges share the same vertex), coincident vertices
//! are merged within `SNAP`, each resulting sub-edge is classified against the other operand by its
//! midpoint (inside / outside / on-boundary, with the on-boundary case resolved by edge direction),
//! and the surviving edges are chained into loops. This handles degenerate input (shared edges,
//! touching vertices, T-junctions, identical operands) deterministically, and is O(n*m), which is
//! plenty for detail-sized polygons.
//!
//! Convention for operands and results: loops are CCW when they bound filled area and CW when they
//! bound a hole.

use crate::geom::{self, Loc, Pt, V2};
use crate::math;
use crate::sort;
use alloc::vec;
use alloc::vec::Vec;
use core::f64::consts::PI;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Intersect,
    Unite,
    Diff,
}

/// Boundary band and vertex-merge distance, inches.
pub const SNAP: f64 = 1e-7;

#[derive(Clone, Copy)]
struct Edge {
    a: V2,
    b: V2,
}

#[derive(Clone, Copy)]
struct Cut {
    t: f64,
    p: V2,
}

#[derive(Clone, Copy)]
struct Sub {
    from: usize,
    to: usize,
    src: u8,
    keep: bool,
}

/// Convert a Region (loop 0 outer, others holes; bulge loops) to oriented flattened loops.
pub fn from_region<L: AsRef<[Pt]>>(loops: &[L], tol: f64) -> Vec<Vec<V2>> {
    loops
        .iter()
        .enumerate()
        .map(|(i, lp)| {
            let mut pts = geom::flatten_loop(lp.as_ref(), tol);
            let area = geom::signed_area_v(&pts);
            if (i == 0 && area < 0.0) || (i > 0 && area > 0.0) {
                pts.reverse();
            }
            pts
        })
        .collect()
}

/// Orient a single loop CCW (filled).
pub fn orient_ccw(pts: &mut [V2]) {
    if geom::signed_area_v(pts) < 0.0 {
        pts.reverse();
    }
}

pub fn total_area<L: AsRef<[V2]>>(loops: &[L]) -> f64 {
    loops.iter().map(|l| geom::signed_area_v(l.as_ref())).sum()
}

pub fn loops_box<L: AsRef<[V2]>>(loops: &[L]) -> geom::Box {
    let mut b = geom::Box::default();
    for l in loops {
        for p in l.as_ref() {
            b.add_point(p.x, p.y);
        }
    }
    b
}

fn clone_loops<L: AsRef<[V2]>>(l: &[L]) -> Vec<Vec<V2>> {
    l.iter().map(|x| x.as_ref().to_vec()).collect()
}

pub fn boolean<LA: AsRef<[V2]>, LB: AsRef<[V2]>>(a: &[LA], b: &[LB], op: Op) -> Vec<Vec<V2>> {
    if a.is_empty() {
        return if op == Op::Unite { clone_loops(b) } else { Vec::new() };
    }
    if b.is_empty() {
        return if op == Op::Intersect { Vec::new() } else { clone_loops(a) };
    }
    let ba = loops_box(a);
    let bb = loops_box(b);
    if !ba.overlaps(bb, SNAP * 4.0) {
        return match op {
            Op::Intersect => Vec::new(),
            Op::Unite => {
                let mut out = clone_loops(a);
                out.extend(clone_loops(b));
                out
            }
            Op::Diff => clone_loops(a),
        };
    }

    let mut edges: Vec<Edge> = Vec::new();
    let mut src: Vec<u8> = Vec::new();
    for l in a {
        append_loop_edges(&mut edges, &mut src, l.as_ref(), 0);
    }
    let na = edges.len();
    for l in b {
        append_loop_edges(&mut edges, &mut src, l.as_ref(), 1);
    }
    let ne = edges.len();

    let mut cuts: Vec<Vec<Cut>> = vec![Vec::new(); ne];

    for i in 0..na {
        let ea = edges[i];
        let bxa = seg_box(ea);
        for j in na..ne {
            let eb = edges[j];
            if !bxa.overlaps(seg_box(eb), SNAP) {
                continue;
            }
            let mut ta = [0.0; 2];
            let mut tb = [0.0; 2];
            let n = geom::seg_seg(ea.a, ea.b, eb.a, eb.b, &mut ta, &mut tb);
            if n == 0 {
                continue;
            }
            if n == 1 {
                let t = ta[0];
                let u = tb[0];
                let p = if t <= 0.0 {
                    ea.a
                } else if t >= 1.0 {
                    ea.b
                } else if u <= 0.0 {
                    eb.a
                } else if u >= 1.0 {
                    eb.b
                } else {
                    ea.a.lerp(ea.b, t)
                };
                cuts[i].push(Cut { t, p });
                cuts[j].push(Cut { t: u, p });
            } else {
                let ab = ea.b.sub(ea.a);
                let l2 = ab.dot(ab);
                let tb0 = eb.a.sub(ea.a).dot(ab) / l2;
                let tb1 = eb.b.sub(ea.a).dot(ab) / l2;
                let p_lo = if ta[0] <= 0.0 {
                    ea.a
                } else if tb0 < tb1 {
                    eb.a
                } else {
                    eb.b
                };
                let p_hi = if ta[1] >= 1.0 {
                    ea.b
                } else if tb0 > tb1 {
                    eb.a
                } else {
                    eb.b
                };
                cuts[i].push(Cut { t: ta[0], p: p_lo });
                cuts[i].push(Cut { t: ta[1], p: p_hi });
                cuts[j].push(Cut { t: tb[0], p: p_lo });
                cuts[j].push(Cut { t: tb[1], p: p_hi });
            }
        }
    }

    let mut verts: Vec<V2> = Vec::new();
    let mut seq_start = vec![0usize; ne + 1];
    let mut seq: Vec<usize> = Vec::new();
    for k in 0..ne {
        seq_start[k] = seq.len();
        let e = edges[k];
        verts.push(e.a);
        seq.push(verts.len() - 1);
        sort::stable(&mut cuts[k], |x, y| x.t < y.t);
        for c in &cuts[k] {
            if c.t <= 0.0 || c.t >= 1.0 {
                continue;
            }
            verts.push(c.p);
            seq.push(verts.len() - 1);
        }
        verts.push(e.b);
        seq.push(verts.len() - 1);
    }
    seq_start[ne] = seq.len();

    let vid = snap_vertices(&verts);
    let nv = vid.iter().map(|&v| v + 1).max().unwrap_or(0);
    let mut pos = vec![V2::init(0.0, 0.0); nv];
    let mut seen = vec![false; nv];
    for (k, &p) in verts.iter().enumerate() {
        let id = vid[k];
        if !seen[id] {
            pos[id] = p;
            seen[id] = true;
        }
    }

    let mut subs: Vec<Sub> = Vec::new();
    for k in 0..ne {
        for q in seq_start[k]..seq_start[k + 1] - 1 {
            let f = vid[seq[q]];
            let t = vid[seq[q + 1]];
            if f == t {
                continue;
            }
            subs.push(Sub { from: f, to: t, src: src[k], keep: false });
        }
    }

    for s in subs.iter_mut() {
        let pa = pos[s.from];
        let pb = pos[s.to];
        let m = pa.mid(pb);
        let loc = if s.src == 0 { geom::locate(m, b, SNAP) } else { geom::locate(m, a, SNAP) };
        let d = pb.sub(pa);
        s.keep = match loc {
            Loc::Inside => match op {
                Op::Intersect => true,
                Op::Unite => false,
                Op::Diff => s.src == 1,
            },
            Loc::Outside => match op {
                Op::Intersect => false,
                Op::Unite => true,
                Op::Diff => s.src == 0,
            },
            Loc::Boundary => {
                if s.src == 1 {
                    false
                } else {
                    let same = same_direction(b, m, d);
                    match op {
                        Op::Intersect | Op::Unite => same,
                        Op::Diff => !same,
                    }
                }
            }
        };
    }
    if op == Op::Diff {
        for s in subs.iter_mut() {
            if s.src == 1 {
                let m = pos[s.from].mid(pos[s.to]);
                let loc = geom::locate(m, a, SNAP);
                s.keep = loc == Loc::Inside;
                if s.keep {
                    core::mem::swap(&mut s.from, &mut s.to);
                }
            }
        }
    }

    let mut out_edges: Vec<Sub> = Vec::new();
    for s in &subs {
        if !s.keep {
            continue;
        }
        if !out_edges.iter().any(|o| o.from == s.from && o.to == s.to) {
            out_edges.push(*s);
        }
    }
    let mut alive = vec![true; out_edges.len()];
    for x in 0..out_edges.len() {
        if !alive[x] {
            continue;
        }
        let e1 = out_edges[x];
        for y in x + 1..out_edges.len() {
            let e2 = out_edges[y];
            if alive[y] && e1.from == e2.to && e1.to == e2.from {
                alive[x] = false;
                alive[y] = false;
                break;
            }
        }
    }
    let fin: Vec<Sub> = out_edges.iter().zip(&alive).filter(|(_, ok)| **ok).map(|(e, _)| *e).collect();

    chain(&pos, &fin)
}

fn append_loop_edges(edges: &mut Vec<Edge>, src: &mut Vec<u8>, l: &[V2], s: u8) {
    if l.len() < 3 {
        return;
    }
    for (i, &p) in l.iter().enumerate() {
        let q = l[(i + 1) % l.len()];
        if p.eql(q, 1e-12) {
            continue;
        }
        edges.push(Edge { a: p, b: q });
        src.push(s);
    }
}

fn seg_box(e: Edge) -> geom::Box {
    geom::Box { x0: e.a.x.min(e.b.x), y0: e.a.y.min(e.b.y), x1: e.a.x.max(e.b.x), y1: e.a.y.max(e.b.y) }
}

/// Direction agreement of a sub-edge (direction `d`, midpoint `m`) with the operand edge it lies on.
fn same_direction<L: AsRef<[V2]>>(loops: &[L], m: V2, d: V2) -> bool {
    let mut best = f64::INFINITY;
    let mut dir = V2::init(0.0, 0.0);
    for lp in loops {
        let lp = lp.as_ref();
        for (i, &p) in lp.iter().enumerate() {
            let q = lp[(i + 1) % lp.len()];
            let dd = geom::dist_point_seg(m, p, q);
            if dd < best {
                best = dd;
                dir = q.sub(p);
            }
        }
    }
    dir.dot(d) > 0.0
}

fn find(par: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while par[r] != r {
        r = par[r];
    }
    let mut c = x;
    while par[c] != r {
        let nx = par[c];
        par[c] = r;
        c = nx;
    }
    r
}

/// Cluster nearby vertices. Returns the cluster id per input vertex (ids are dense, in first-seen order).
fn snap_vertices(v: &[V2]) -> Vec<usize> {
    let n = v.len();
    let mut order: Vec<usize> = (0..n).collect();
    sort::stable(&mut order, |&x, &y| {
        let (p, q) = (v[x], v[y]);
        if p.x != q.x {
            return p.x < q.x;
        }
        if p.y != q.y {
            return p.y < q.y;
        }
        x < y
    });
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        let pi = v[order[i]];
        let mut j = i + 1;
        while j < n && v[order[j]].x - pi.x <= SNAP {
            let pj = v[order[j]];
            if (pj.y - pi.y).abs() <= SNAP {
                let ra = find(&mut parent, order[i]);
                let rb = find(&mut parent, order[j]);
                if ra != rb {
                    if ra < rb {
                        parent[rb] = ra;
                    } else {
                        parent[ra] = rb;
                    }
                }
            }
            j += 1;
        }
    }
    let mut ids = vec![0usize; n];
    let mut root_id = vec![usize::MAX; n];
    let mut next = 0;
    for (k, id) in ids.iter_mut().enumerate() {
        let r = find(&mut parent, k);
        if root_id[r] == usize::MAX {
            root_id[r] = next;
            next += 1;
        }
        *id = root_id[r];
    }
    ids
}

fn chain(pos: &[V2], edges: &[Sub]) -> Vec<Vec<V2>> {
    let ne = edges.len();
    let mut used = vec![false; ne];
    let mut by_from: Vec<usize> = (0..ne).collect();
    sort::stable(&mut by_from, |&x, &y| {
        if edges[x].from != edges[y].from {
            return edges[x].from < edges[y].from;
        }
        x < y
    });
    let mut first = vec![0usize; pos.len() + 1];
    for e in edges {
        first[e.from + 1] += 1;
    }
    for k in 1..first.len() {
        first[k] += first[k - 1];
    }

    let mut loops: Vec<Vec<V2>> = Vec::new();
    for si in 0..ne {
        if used[si] {
            continue;
        }
        let mut pts: Vec<V2> = Vec::new();
        let start_v = edges[si].from;
        let mut cur = si;
        let mut closed = false;
        for _ in 0..=ne {
            used[cur] = true;
            pts.push(pos[edges[cur].from]);
            let v = edges[cur].to;
            if v == start_v {
                closed = true;
                break;
            }
            let din = pos[v].sub(pos[edges[cur].from]);
            let back = din.scale(-1.0);
            let mut best: Option<usize> = None;
            let mut best_ang = f64::INFINITY;
            for &ei in &by_from[first[v]..first[v + 1]] {
                if used[ei] {
                    continue;
                }
                let d = pos[edges[ei].to].sub(pos[v]);
                let mut ang = -math::atan2(back.cross(d), back.dot(d));
                if ang <= 1e-12 {
                    ang += 2.0 * PI;
                }
                if ang < best_ang {
                    best_ang = ang;
                    best = Some(ei);
                }
            }
            match best {
                Some(b) => cur = b,
                None => break,
            }
        }
        if !closed || pts.len() < 3 {
            continue;
        }
        let cleaned = clean_loop(&pts);
        if cleaned.len() < 3 || geom::signed_area_v(&cleaned).abs() < 1e-10 {
            continue;
        }
        loops.push(cleaned);
    }
    loops
}

/// Drop duplicate and collinear vertices.
fn clean_loop(pts: &[V2]) -> Vec<V2> {
    let mut cur = pts.to_vec();
    let mut changed = true;
    while changed && cur.len() >= 3 {
        changed = false;
        let n = cur.len();
        let mut out = Vec::new();
        for (i, &p) in cur.iter().enumerate() {
            let prev = cur[(i + n - 1) % n];
            let next = cur[(i + 1) % n];
            let area2 = (p.x - prev.x) * (next.y - prev.y) - (p.y - prev.y) * (next.x - prev.x);
            let span = prev.dist(next);
            let near_dup = p.eql(prev, SNAP);
            if near_dup || (span > 0.0 && area2.abs() / span < SNAP && p.sub(prev).dot(next.sub(p)) > 0.0) {
                changed = true;
                continue;
            }
            out.push(p);
        }
        cur = out;
    }
    cur
}
