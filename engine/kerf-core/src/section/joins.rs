//! Monolithic concrete (SPEC 28.7): where two bodies of one casting touch, the edge they share is not a joint and is
//! not drawn. One casting is the slabs of a member a void or a footprint split along z, and the concrete components
//! that name the same `pour`.

use super::occlusion::{Occ, PieceOut};
use super::{Class, Section};
use crate::geom::{self, Pt, V2};
use crate::model::{Kind, Prism};
use alloc::vec::Vec;

impl Section<'_> {
    /// The `pour` group of a prism's component (concrete only).
    fn pour_of(&self, p: &Prism) -> Option<&str> {
        let c = self.scene.comps.get(p.comp as usize)?;
        if c.ty.name() != "concrete" {
            return None;
        }
        c.node.get("pour").and_then(crate::json::Value::str).filter(|s| !s.is_empty())
    }

    fn joins_with(&self, i: usize, j: usize) -> bool {
        let (p, q) = (&self.prisms[i], &self.prisms[j]);
        if self.cls[i] != self.cls[j] || p.material != q.material || q.kind != Kind::Body || q.embedded || q.dashed {
            return false;
        }
        if self.cls[i] == Class::Beyond && (p.z1 - q.z1).abs() > 1e-6 {
            return false;
        }
        let same_member =
            p.comp == q.comp && p.instance == q.instance && (p.seam.contains(&true) || q.seam.contains(&true));
        same_member || self.pour_of(p).is_some_and(|a| self.pour_of(q) == Some(a))
    }

    /// Per prism, the edges of the bodies it is cast with; empty for most prisms.
    pub(super) fn join_edges(&mut self) -> Vec<Vec<[V2; 2]>> {
        let n = self.prisms.len();
        let mut out: Vec<Vec<[V2; 2]>> = alloc::vec![Vec::new(); n];
        let cand: Vec<usize> = (0..n)
            .filter(|&i| {
                let p = &self.prisms[i];
                self.cls[i] != Class::Drop
                    && p.kind == Kind::Body
                    && !p.embedded
                    && !p.dashed
                    && (p.seam.contains(&true) || self.pour_of(p).is_some())
            })
            .collect();
        for &i in &cand {
            let bi = crate::clip::loops_box(&self.flat_of(i));
            for &j in &cand {
                if i == j || !self.joins_with(i, j) {
                    continue;
                }
                let fj = self.flat_of(j);
                if !crate::clip::loops_box(&fj).overlaps(bi, 1e-6) {
                    continue;
                }
                for lp in fj.iter() {
                    for (k, a) in lp.iter().enumerate() {
                        out[i].push([*a, lp[(k + 1) % lp.len()]]);
                    }
                }
            }
        }
        out
    }

    /// Bodies cast together also hide together: a line behind exactly where two of them meet is inside their union, not
    /// on an edge of either, so each joined pair adds its union as one occluder.
    pub(super) fn join_occluders(&mut self) {
        let mut add: Vec<Occ> = Vec::new();
        for o in &self.occs {
            for q in &self.occs {
                if q.prism <= o.prism || o.cut != q.cut || !o.bx.overlaps(q.bx, 1e-6) {
                    continue;
                }
                if !self.joins.get(o.prism).is_some_and(|e| !e.is_empty()) || !self.joins_with(o.prism, q.prism) {
                    continue;
                }
                let loops = crate::clip::boolean(&o.loops, &q.loops, crate::clip::Op::Unite);
                let bx = crate::clip::loops_box(&loops);
                add.push(Occ { loops: alloc::rc::Rc::new(loops), bx, z1: o.z1.max(q.z1), cut: o.cut, prism: o.prism });
            }
        }
        self.occs.extend(add);
    }
}

/// The pieces of `pieces` that do not lie along `edges` (arcs are kept whole).
pub fn off_edges(pieces: Vec<PieceOut>, edges: &[[V2; 2]]) -> Vec<PieceOut> {
    if edges.is_empty() {
        return pieces;
    }
    let mut out = Vec::new();
    for pc in pieces {
        let n = pc.pts.len();
        let nseg = if pc.closed { n } else { n.saturating_sub(1) };
        let mut cur: Vec<Pt> = Vec::new();
        for k in 0..nseg {
            let (a, b) = (pc.pts[k], pc.pts[(k + 1) % n]);
            if a.b != 0.0 {
                flush(&mut cur, &mut out);
                out.push(PieceOut { pts: alloc::vec![a, Pt::at(b.v(), 0.0)], closed: false });
                continue;
            }
            for iv in geom::uncovered(a.v(), b.v(), edges) {
                let (s, e) = (a.v().lerp(b.v(), iv[0]), a.v().lerp(b.v(), iv[1]));
                if cur.last().is_none_or(|l| !l.v().eql(s, 1e-9)) {
                    flush(&mut cur, &mut out);
                    cur.push(Pt::at(s, 0.0));
                }
                cur.push(Pt::at(e, 0.0));
            }
        }
        flush(&mut cur, &mut out);
    }
    out
}

fn flush(cur: &mut Vec<Pt>, out: &mut Vec<PieceOut>) {
    if cur.len() >= 2 {
        out.push(PieceOut { pts: core::mem::take(cur), closed: false });
    }
    cur.clear();
}
