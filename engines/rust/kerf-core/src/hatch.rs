//! Hatch pattern line generation: AutoCAD .pat families clipped to a region (even-odd).

use crate::geom::*;
use crate::style::PatLine;

const MAX_LINES_PER_FAMILY: usize = 6000;
const MAX_SEGMENTS: usize = 60000;

/// Liang-Barsky clip of a line segment to a rect.
pub fn clip_line_rect(a: Pt, b: Pt, r: &Rect) -> Option<(Pt, Pt)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = b - a;
    for (p, q) in [(-d.x, a.x - r.x0), (d.x, r.x1 - a.x), (-d.y, a.y - r.y0), (d.y, r.y1 - a.y)] {
        if p.abs() < 1e-15 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return None;
                }
                t1 = t1.min(t);
            }
        }
    }
    if t1 < t0 {
        return None;
    }
    Some((a + d * t0, a + d * t1))
}

/// Generate hatch segments for `loops` (polygons, even-odd). `k` = paper-to-model factor
/// (pattern scale * view scale). Output segments clipped to `clip` when given.
pub fn hatch_lines(loops: &[Vec<Pt>], pat: &[PatLine], k: f64, angle_deg: f64, clip: Option<&Rect>) -> Vec<[f64; 4]> {
    let mut out: Vec<[f64; 4]> = vec![];
    let mut bb = Rect::empty();
    for l in loops {
        for p in l {
            bb.add(*p);
        }
    }
    if bb.is_empty() {
        return out;
    }
    if let Some(c) = clip {
        if !bb.overlaps(c, 0.0) {
            return out;
        }
        // shrink region of interest to the clip window
        bb = Rect::new(bb.x0.max(c.x0), bb.y0.max(c.y0), bb.x1.min(c.x1), bb.y1.min(c.y1));
        if bb.is_empty() {
            return out;
        }
    }
    let rot0 = angle_deg.to_radians();
    for fam in pat {
        let ang = (fam.angle + angle_deg).to_radians();
        let u = pt(ang.cos(), ang.sin());
        let vv = pt(-ang.sin(), ang.cos());
        let o = pt(fam.x0, fam.y0).rot(rot0) * k;
        let dxs = fam.dx * k;
        let dys = fam.dy * k;
        if dys.abs() < 1e-9 {
            continue;
        }
        let (o_s, o_t) = (o.dot(u), o.dot(vv));
        // region extents in frame
        let (mut tmin, mut tmax) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in bb.corners() {
            let t = c.dot(vv);
            tmin = tmin.min(t);
            tmax = tmax.max(t);
        }
        let n_lo = ((tmin - o_t) / dys).min((tmax - o_t) / dys).floor() as i64;
        let n_hi = ((tmin - o_t) / dys).max((tmax - o_t) / dys).ceil() as i64;
        if (n_hi - n_lo) as usize > MAX_LINES_PER_FAMILY {
            continue;
        }
        let dashes: Vec<f64> = fam.dashes.iter().map(|d| d * k).collect();
        let period: f64 = dashes.iter().map(|d| d.abs()).sum();
        for n in n_lo..=n_hi {
            let t = o_t + n as f64 * dys;
            if t < tmin - 1e-9 || t > tmax + 1e-9 {
                continue;
            }
            // crossings of the line (in frame) with each loop edge
            let mut xs: Vec<f64> = vec![];
            for l in loops {
                let m = l.len();
                for i in 0..m {
                    let a = l[i];
                    let b = l[(i + 1) % m];
                    let (at, bt) = (a.dot(vv), b.dot(vv));
                    // half-open rule to avoid double counting vertices
                    if (at <= t) != (bt <= t) {
                        let f = (t - at) / (bt - at);
                        let p = a.lerp(b, f);
                        xs.push(p.dot(u));
                    }
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let phase = o_s + n as f64 * dxs;
            let mut i = 0;
            while i + 1 < xs.len() {
                let (s0, s1) = (xs[i], xs[i + 1]);
                i += 2;
                if s1 - s0 < 1e-9 {
                    continue;
                }
                let mut emit = |sa: f64, sb: f64, out: &mut Vec<[f64; 4]>| {
                    let pa = u * sa + vv * t;
                    let pb = u * sb + vv * t;
                    if let Some(c) = clip {
                        if sa == sb {
                            if c.contains(pa, 1e-9) {
                                out.push([pa.x, pa.y, pa.x, pa.y]);
                            }
                        } else if let Some((qa, qb)) = clip_line_rect(pa, pb, c) {
                            out.push([qa.x, qa.y, qb.x, qb.y]);
                        }
                    } else {
                        out.push([pa.x, pa.y, pb.x, pb.y]);
                    }
                };
                if dashes.is_empty() || period < 1e-9 {
                    emit(s0, s1, &mut out);
                } else {
                    // walk the dash pattern from `phase`
                    let rel0 = s0 - phase;
                    let cycles = (rel0 / period).floor();
                    let mut pos = phase + cycles * period;
                    let mut guard = 0;
                    'walk: loop {
                        for d in &dashes {
                            let len = d.abs();
                            let (a, b) = (pos, pos + len);
                            if *d >= 0.0 {
                                if len < 1e-12 {
                                    // dot
                                    if a >= s0 - 1e-9 && a <= s1 + 1e-9 {
                                        emit(a, a, &mut out);
                                    }
                                } else {
                                    let (ca, cb) = (a.max(s0), b.min(s1));
                                    if cb - ca > 1e-9 {
                                        emit(ca, cb, &mut out);
                                    }
                                }
                            }
                            pos = b;
                            if pos > s1 + 1e-9 {
                                break 'walk;
                            }
                            guard += 1;
                            if guard > 200000 || out.len() > MAX_SEGMENTS {
                                break 'walk;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::PatLine;

    #[test]
    fn simple_ansi31() {
        let sq = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        let pat = vec![PatLine { angle: 45.0, x0: 0.0, y0: 0.0, dx: 0.0, dy: 0.125, dashes: vec![] }];
        let lines = hatch_lines(&[sq], &pat, 1.0, 0.0, None);
        assert!(lines.len() > 100);
        for l in &lines {
            let dx = l[2] - l[0];
            let dy = l[3] - l[1];
            assert!((dx - dy).abs() < 1e-6);
        }
    }

    #[test]
    fn dotted() {
        let sq = vec![pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0), pt(0.0, 1.0)];
        let pat = vec![PatLine { angle: 0.0, x0: 0.0, y0: 0.0, dx: 0.1, dy: 0.1, dashes: vec![0.0, -0.2] }];
        let lines = hatch_lines(&[sq], &pat, 1.0, 0.0, None);
        assert!(lines.iter().all(|l| l[0] == l[2] && l[1] == l[3]));
        assert!(lines.len() > 10);
    }
}
