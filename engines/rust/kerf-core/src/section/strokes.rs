//! Strokes: pen-tagged polylines, coincident-edge deduplication and end-to-end chaining.

use crate::geom::{self, Box, Pt, V2};
use crate::pen::Pen;
use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug)]
pub struct Stroke {
    pub pts: Vec<Pt>,
    pub closed: bool,
    pub pen: Pen,
    pub src: String,
    pub rank: f64,
    pub seq: u32,
}

struct SegRef {
    a: V2,
    b: V2,
    bulge: f64,
    stroke: usize,
    keep: Vec<[f64; 2]>,
}

pub const DEDUPE_TOL: f64 = 1e-4;

fn seg_box(s: &SegRef) -> Box {
    Box { x0: s.a.x.min(s.b.x), y0: s.a.y.min(s.b.y), x1: s.a.x.max(s.b.x), y1: s.a.y.max(s.b.y) }
}

/// Overlapping collinear segments: the lighter pen loses the shared span; members of one component keep one thin copy.
pub fn dedupe(strokes: &[Stroke], style: &Style) -> Vec<Stroke> {
    let mut segs: Vec<SegRef> = Vec::new();
    let mut seg_start: Vec<usize> = Vec::with_capacity(strokes.len() + 1);
    for (si, s) in strokes.iter().enumerate() {
        seg_start.push(segs.len());
        let nseg = if s.closed { s.pts.len() } else { s.pts.len().saturating_sub(1) };
        for k in 0..nseg {
            let p = s.pts[k];
            let q = s.pts[(k + 1) % s.pts.len()];
            segs.push(SegRef { a: p.v(), b: q.v(), bulge: p.b, stroke: si, keep: alloc::vec![[0.0, 1.0]] });
        }
    }
    seg_start.push(segs.len());
    let mut extras: Vec<Stroke> = Vec::new();
    for i in 0..segs.len() {
        let (head, tail) = segs.split_at_mut(i + 1);
        let s1 = &mut head[i];
        if s1.bulge != 0.0 {
            continue;
        }
        let d1 = s1.b.sub(s1.a);
        let l1 = d1.len();
        if l1 < 1e-9 {
            continue;
        }
        let ud1 = d1.scale(1.0 / l1);
        let w1 = style.pen_width_mm(strokes[s1.stroke].pen);
        let bb1 = seg_box(s1);
        for s2 in tail.iter_mut() {
            if s2.bulge != 0.0 {
                continue;
            }
            let bb2 = seg_box(s2);
            if !bb1.overlaps(bb2, DEDUPE_TOL) {
                continue;
            }
            let d2 = s2.b.sub(s2.a);
            let l2 = d2.len();
            if l2 < 1e-9 {
                continue;
            }
            if ud1.cross(d2).abs() / l2 > 1e-6 && ud1.cross(d2).abs() > DEDUPE_TOL {
                continue;
            }
            let off_a = ud1.cross(s2.a.sub(s1.a)).abs();
            let off_b = ud1.cross(s2.b.sub(s1.a)).abs();
            if off_a > DEDUPE_TOL || off_b > DEDUPE_TOL {
                continue;
            }
            let ta = s2.a.sub(s1.a).dot(ud1) / l1;
            let tb = s2.b.sub(s1.a).dot(ud1) / l1;
            let lo = 0.0f64.max(ta.min(tb));
            let hi = 1.0f64.min(ta.max(tb));
            if hi - lo < DEDUPE_TOL / l1 {
                continue;
            }
            let w2 = style.pen_width_mm(strokes[s2.stroke].pen);
            let same_base = s1.stroke != s2.stroke && base_id(&strokes[s1.stroke].src) == base_id(&strokes[s2.stroke].src);
            let p_lo = s1.a.add(d1.scale(lo));
            let p_hi = s1.a.add(d1.scale(hi));
            let t2a = p_lo.sub(s2.a).dot(d2) / (l2 * l2);
            let t2b = p_hi.sub(s2.a).dot(d2) / (l2 * l2);
            if same_base {
                subtract_interval(&mut s1.keep, lo, hi);
                subtract_interval(&mut s2.keep, 0.0f64.max(t2a.min(t2b)), 1.0f64.min(t2a.max(t2b)));
                extras.push(Stroke {
                    pts: alloc::vec![Pt::at(p_lo, 0.0), Pt::at(p_hi, 0.0)],
                    closed: false,
                    pen: Pen::Beyond,
                    src: strokes[s1.stroke].src.clone(),
                    rank: style.pen_width_mm(Pen::Beyond),
                    seq: strokes[s1.stroke].seq,
                });
                continue;
            }
            if w1 < w2 {
                subtract_interval(&mut s1.keep, lo, hi);
            } else {
                subtract_interval(&mut s2.keep, 0.0f64.max(t2a.min(t2b)), 1.0f64.min(t2a.max(t2b)));
            }
        }
    }
    let mut out: Vec<Stroke> = Vec::new();
    for (si, s) in strokes.iter().enumerate() {
        let items = &segs[seg_start[si]..seg_start[si + 1]];
        let untouched = items.iter().all(|sr| sr.keep.len() == 1 && sr.keep[0][0] == 0.0 && sr.keep[0][1] == 1.0);
        if untouched {
            out.push(s.clone());
            continue;
        }
        let mut cur: Vec<Pt> = Vec::new();
        let flush = |cur: &mut Vec<Pt>, out: &mut Vec<Stroke>| {
            out.push(Stroke { pts: core::mem::take(cur), closed: false, pen: s.pen, src: s.src.clone(), rank: s.rank, seq: s.seq });
        };
        for sr in items {
            for iv in &sr.keep {
                let sub = geom::sub_seg(sr.a, sr.b, sr.bulge, iv[0], iv[1]);
                if sub.a.dist(sub.b) < 1e-9 {
                    continue;
                }
                let joins = cur.last().is_some_and(|l| l.v().eql(sub.a, 1e-7));
                if !joins {
                    if cur.len() > 1 {
                        flush(&mut cur, &mut out);
                    }
                    cur.clear();
                    cur.push(Pt { x: sub.a.x, y: sub.a.y, b: sub.bulge });
                } else if let Some(last) = cur.last_mut() {
                    last.b = sub.bulge;
                }
                cur.push(Pt { x: sub.b.x, y: sub.b.y, b: 0.0 });
            }
        }
        if cur.len() > 1 {
            flush(&mut cur, &mut out);
        }
    }
    out.extend(extras);
    out
}

fn subtract_interval(keep: &mut Vec<[f64; 2]>, lo: f64, hi: f64) {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for &iv in keep.iter() {
        if hi <= iv[0] || lo >= iv[1] {
            out.push(iv);
            continue;
        }
        if lo > iv[0] {
            out.push([iv[0], lo]);
        }
        if hi < iv[1] {
            out.push([hi, iv[1]]);
        }
    }
    *keep = out;
}

/// Joins open strokes of the same pen and source that meet end to end; closes loops that return to their start.
pub fn chain_strokes(input: Vec<Stroke>) -> Vec<Stroke> {
    let mut list = input;
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < list.len() {
            if list[i].closed {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < list.len() {
                let (si, sj) = (&list[i], &list[j]);
                if sj.closed || si.pen != sj.pen || si.src != sj.src {
                    j += 1;
                    continue;
                }
                let (Some(i_last), Some(j_last)) = (si.pts.last(), sj.pts.last()) else {
                    j += 1;
                    continue;
                };
                let i_end = i_last.v();
                let i_start = si.pts[0].v();
                let j_start = sj.pts[0].v();
                let j_end = j_last.v();
                let merged = if i_end.eql(j_start, 1e-7) {
                    let mut m = si.pts.clone();
                    if let Some(l) = m.last_mut() {
                        l.b = sj.pts[0].b;
                    }
                    m.extend_from_slice(&sj.pts[1..]);
                    Some(m)
                } else if j_end.eql(i_start, 1e-7) {
                    let mut m = sj.pts.clone();
                    if let Some(l) = m.last_mut() {
                        l.b = si.pts[0].b;
                    }
                    m.extend_from_slice(&si.pts[1..]);
                    Some(m)
                } else {
                    None
                };
                if let Some(mp) = merged {
                    let seq = si.seq.min(sj.seq);
                    list[i].pts = mp;
                    list[i].seq = seq;
                    list.remove(j);
                    changed = true;
                    break;
                }
                j += 1;
            }
            i += 1;
        }
    }
    for st in list.iter_mut() {
        let n = st.pts.len();
        if !st.closed && n >= 4 && st.pts[0].v().eql(st.pts[n - 1].v(), 1e-7) && st.pts[n - 1].b == 0.0 {
            st.pts.truncate(n - 1);
            st.closed = true;
        }
    }
    list
}

/// The component id of a source name (`"truss#2"` is `"truss"`).
pub fn base_id(src: &str) -> &str {
    src.split('#').next().unwrap_or(src)
}
