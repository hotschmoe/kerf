//! Visibility of linework against the flattened cut / beyond bodies in front of it.

use crate::geom::{self, Box, Pt, V2};
use crate::sort;
use alloc::rc::Rc;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct Occ {
    pub loops: Rc<Vec<Vec<V2>>>,
    pub bx: Box,
    pub z1: f64,
    pub cut: bool,
    pub prism: usize,
}

#[derive(Clone, Debug)]
pub struct PieceOut {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

fn push_unique(ts: &mut Vec<f64>, t: f64) {
    if t > 1e-9 && t < 1.0 - 1e-9 {
        ts.push(t);
    }
}

/// Visible parts of a closed loop; the first and last piece are joined when the loop start is visible.
pub fn visible_pieces(lp: &[Pt], occ: &[&Occ]) -> Vec<PieceOut> {
    visible_impl(lp, true, true, occ)
}

pub fn visible_open(pts: &[Pt], closed: bool, occ: &[&Occ]) -> Vec<PieceOut> {
    visible_impl(pts, closed, false, occ)
}

fn visible_impl(pts: &[Pt], closed: bool, join_wrap: bool, occ: &[&Occ]) -> Vec<PieceOut> {
    let mut out: Vec<PieceOut> = Vec::new();
    let n = pts.len();
    let nseg = if closed { n } else { n.saturating_sub(1) };
    let mut cur: Vec<Pt> = Vec::new();
    let mut any_hidden = false;
    let mut starts_visible_at_0 = false;
    let mut ts: Vec<f64> = Vec::new();
    for i in 0..nseg {
        let p0 = pts[i].v();
        let p1 = pts[(i + 1) % n].v();
        let bulge = pts[i].b;
        let mut sbox = Box::default();
        geom::seg_box_into(&mut sbox, p0, p1, bulge);
        ts.clear();
        for o in occ {
            if !o.bx.overlaps(sbox, 1e-7) {
                continue;
            }
            for ol in o.loops.iter() {
                for (k, &q0) in ol.iter().enumerate() {
                    let q1 = ol[(k + 1) % ol.len()];
                    let mut ta = [0.0; 2];
                    let mut tb = [0.0; 2];
                    let cnt = if bulge == 0.0 {
                        geom::seg_seg(p0, p1, q0, q1, &mut ta, &mut tb)
                    } else {
                        geom::arc_seg(p0, p1, bulge, q0, q1, &mut ta, &mut tb)
                    };
                    for &t in ta.iter().take(cnt) {
                        push_unique(&mut ts, t);
                    }
                }
            }
        }
        sort::stable(&mut ts, |a, b| a < b);
        let mut t_prev = 0.0;
        for k in 0..=ts.len() {
            let t_next = if k < ts.len() { ts[k] } else { 1.0 };
            if t_next - t_prev < 1e-10 {
                t_prev = t_next;
                continue;
            }
            let sub = geom::sub_seg(p0, p1, bulge, t_prev, t_next);
            let mid = geom::seg_point(p0, p1, bulge, (t_prev + t_next) / 2.0);
            let hidden =
                occ.iter().any(|o| o.bx.contains(mid) && geom::locate(mid, &o.loops, 1e-7) == geom::Loc::Inside);
            if hidden {
                any_hidden = true;
                if let Some(last) = cur.last_mut() {
                    last.b = 0.0;
                    out.push(PieceOut { pts: core::mem::take(&mut cur), closed: false });
                }
            } else {
                let joins = cur.last().is_some_and(|l| l.v().eql(sub.a, 1e-9));
                if !joins {
                    if cur.len() > 1 {
                        if let Some(last) = cur.last_mut() {
                            last.b = 0.0;
                        }
                        out.push(PieceOut { pts: core::mem::take(&mut cur), closed: false });
                    }
                    cur.clear();
                    cur.push(Pt { x: sub.a.x, y: sub.a.y, b: sub.bulge });
                    if i == 0 && t_prev == 0.0 {
                        starts_visible_at_0 = true;
                    }
                } else if let Some(last) = cur.last_mut() {
                    last.b = sub.bulge;
                }
                cur.push(Pt { x: sub.b.x, y: sub.b.y, b: 0.0 });
            }
            t_prev = t_next;
        }
    }
    if !any_hidden {
        return alloc::vec![PieceOut { pts: pts.to_vec(), closed }];
    }
    if cur.len() > 1 {
        if join_wrap && starts_visible_at_0 && out.first().is_some_and(|p| !p.pts.is_empty()) {
            let head = out.remove(0);
            let mut joined = cur;
            if let Some(last) = joined.last_mut() {
                last.b = head.pts[0].b;
            }
            joined.extend_from_slice(&head.pts[1..]);
            out.push(PieceOut { pts: joined, closed: false });
        } else {
            out.push(PieceOut { pts: cur, closed: false });
        }
    }
    out
}
