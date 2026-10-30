//! Clip bulge polylines (lines and arcs, exactly) to an axis-aligned box, and scan loops against
//! axis-parallel lines.

use crate::geom::{self, Box, Pt, V2};
use crate::sort;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct Piece {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

const TOL: f64 = 1e-9;

fn in_box(b: &Box, p: V2) -> bool {
    p.x >= b.x0 - TOL && p.x <= b.x1 + TOL && p.y >= b.y0 - TOL && p.y <= b.y1 + TOL
}

fn clip_line(p: V2, q: V2, b: &Box) -> Option<[f64; 2]> {
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let d = q.sub(p);
    let ps = [-d.x, d.x, -d.y, d.y];
    let qs = [p.x - b.x0, b.x1 - p.x, p.y - b.y0, b.y1 - p.y];
    for i in 0..4 {
        if ps[i].abs() < 1e-15 {
            if qs[i] < -TOL {
                return None;
            }
        } else {
            let r = qs[i] / ps[i];
            if ps[i] < 0.0 {
                if r > t1 {
                    return None;
                }
                if r > t0 {
                    t0 = r;
                }
            } else {
                if r < t0 {
                    return None;
                }
                if r < t1 {
                    t1 = r;
                }
            }
        }
    }
    if t1 - t0 < 1e-12 {
        return None;
    }
    Some([t0, t1])
}

type Interval = [f64; 2];

fn clip_arc(p0: V2, p1: V2, bulge: f64, b: &Box) -> Vec<Interval> {
    let mut ts: Vec<f64> = alloc::vec![0.0, 1.0];
    let corners = [V2::init(b.x0, b.y0), V2::init(b.x1, b.y0), V2::init(b.x1, b.y1), V2::init(b.x0, b.y1)];
    for i in 0..4 {
        let mut ta = [0.0; 2];
        let mut tb = [0.0; 2];
        let n = geom::arc_seg(p0, p1, bulge, corners[i], corners[(i + 1) % 4], &mut ta, &mut tb);
        ts.extend_from_slice(&ta[..n]);
    }
    sort::stable(&mut ts, |a, b| a < b);
    let mut out: Vec<Interval> = Vec::new();
    for w in ts.windows(2) {
        let (t0, t1) = (w[0], w[1]);
        if t1 - t0 < 1e-10 {
            continue;
        }
        let m = geom::seg_point(p0, p1, bulge, (t0 + t1) / 2.0);
        if in_box(b, m) {
            match out.last_mut() {
                Some(last) if (last[1] - t0).abs() < 1e-10 => last[1] = t1,
                _ => out.push([t0, t1]),
            }
        }
    }
    out
}

pub fn clip_path(pts: &[Pt], closed: bool, bx: Box) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    if pts.len() < 2 {
        return pieces;
    }
    let nseg = if closed { pts.len() } else { pts.len() - 1 };
    let mut all_inside = true;
    for (i, p) in pts.iter().enumerate() {
        if !in_box(&bx, p.v()) {
            all_inside = false;
            break;
        }
        if p.b != 0.0 && (closed || i + 1 < pts.len()) {
            let q = pts[(i + 1) % pts.len()];
            let mut bb = Box::default();
            geom::seg_box_into(&mut bb, p.v(), q.v(), p.b);
            if !in_box(&bx, V2::init(bb.x0, bb.y0)) || !in_box(&bx, V2::init(bb.x1, bb.y1)) {
                all_inside = false;
                break;
            }
        }
    }
    if all_inside {
        pieces.push(Piece { pts: pts.to_vec(), closed });
        return pieces;
    }
    let mut cur: Vec<Pt> = Vec::new();
    let mut starts_at_zero_first = false;
    let mut last_end_full = false;
    for i in 0..nseg {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        let ivs: Vec<Interval> = if p.b == 0.0 {
            clip_line(p.v(), q.v(), &bx).into_iter().collect()
        } else {
            clip_arc(p.v(), q.v(), p.b, &bx)
        };
        if ivs.is_empty() {
            if !cur.is_empty() {
                flush(&mut pieces, &mut cur);
            }
            last_end_full = false;
            continue;
        }
        for (k, iv) in ivs.iter().enumerate() {
            let sub = geom::sub_seg(p.v(), q.v(), p.b, iv[0], iv[1]);
            let continues = k == 0 && last_end_full && !cur.is_empty() && iv[0] <= 1e-12;
            if !continues {
                if !cur.is_empty() {
                    flush(&mut pieces, &mut cur);
                }
                if i == 0 && k == 0 && iv[0] <= 1e-12 {
                    starts_at_zero_first = true;
                }
                cur.push(Pt { x: sub.a.x, y: sub.a.y, b: sub.bulge });
            } else if let Some(last) = cur.last_mut() {
                last.b = sub.bulge;
            }
            cur.push(Pt { x: sub.b.x, y: sub.b.y, b: 0.0 });
            last_end_full = k + 1 == ivs.len() && iv[1] >= 1.0 - 1e-12;
            if k + 1 < ivs.len() {
                flush(&mut pieces, &mut cur);
            }
        }
    }
    if !cur.is_empty() {
        if closed && last_end_full && starts_at_zero_first && !pieces.is_empty() {
            let head = pieces.remove(0);
            let mut joined = core::mem::take(&mut cur);
            if let (Some(last), Some(h0)) = (joined.last_mut(), head.pts.first()) {
                last.b = h0.b;
            }
            joined.extend_from_slice(head.pts.get(1..).unwrap_or(&[]));
            pieces.push(Piece { pts: joined, closed: false });
        } else {
            flush(&mut pieces, &mut cur);
        }
    }
    pieces
}

fn flush(pieces: &mut Vec<Piece>, cur: &mut Vec<Pt>) {
    if cur.len() >= 2 {
        if let Some(last) = cur.last_mut() {
            last.b = 0.0;
        }
        pieces.push(Piece { pts: core::mem::take(cur), closed: false });
    }
    cur.clear();
}

pub fn clip_seg(p: V2, q: V2, bx: Box) -> Option<[V2; 2]> {
    let iv = clip_line(p, q, &bx)?;
    Some([p.lerp(q, iv[0]), p.lerp(q, iv[1])])
}

pub fn scan<L: AsRef<[V2]>>(loops: &[L], c: f64, vertical: bool) -> Vec<f64> {
    let mut out = Vec::new();
    for l in loops {
        let l = l.as_ref();
        for (i, &p) in l.iter().enumerate() {
            let q = l[(i + 1) % l.len()];
            let (pa, qa) = if vertical { (p.x, q.x) } else { (p.y, q.y) };
            if (pa < c) != (qa < c) {
                let t = (c - pa) / (qa - pa);
                let (pb, qb) = if vertical { (p.y, q.y) } else { (p.x, q.x) };
                out.push(pb + (qb - pb) * t);
            }
        }
    }
    sort::stable(&mut out, |a, b| a < b);
    out
}
