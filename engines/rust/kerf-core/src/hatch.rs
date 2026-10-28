//! Hatch pattern line generation (AutoCAD .pat semantics) clipped to a region (even-odd).

use crate::geom::{self, Loc, RAD_PER_DEG, V2};
use crate::math;
use crate::num;
use crate::sort;
use crate::style::Pattern;
use alloc::vec::Vec;
use core::f64::consts::PI;

pub type Line = [f64; 4];

pub const MAX_LINES: usize = 150_000;

pub struct HatchLines {
    pub lines: Vec<Line>,
    pub truncated: bool,
}

pub fn generate<L: AsRef<[V2]>>(loops: &[L], pattern: &Pattern, k: f64, rot_deg: f64) -> HatchLines {
    let mut lines: Vec<Line> = Vec::new();
    let mut truncated = false;
    let rot = rot_deg * RAD_PER_DEG;
    let cr = math::cos(rot);
    let sr = math::sin(rot);
    for fam in &pattern.families {
        let th = fam.angle * RAD_PER_DEG + rot;
        let dir = V2::init(math::cos(th), math::sin(th));
        let nrm = V2::init(-dir.y, dir.x);
        let ox = (fam.x0 * cr - fam.y0 * sr) * k;
        let oy = (fam.x0 * sr + fam.y0 * cr) * k;
        let o = V2::init(ox, oy);
        let dy = fam.dy * k;
        let dx = fam.dx * k;
        if dy.abs() < 1e-9 {
            continue;
        }
        let mut smin = f64::INFINITY;
        let mut smax = f64::NEG_INFINITY;
        for l in loops {
            for p in l.as_ref() {
                let s = p.dot(nrm);
                smin = smin.min(s);
                smax = smax.max(s);
            }
        }
        if smin > smax {
            continue;
        }
        let s0 = o.dot(nrm);
        let qa = (smin - s0) / dy;
        let qb = (smax - s0) / dy;
        let lo_f = math::ceil(qa.min(qb) - 1e-9);
        let hi_f = math::floor(qa.max(qb) + 1e-9);
        if lo_f > hi_f {
            continue;
        }
        if !(hi_f - lo_f + 1.0 <= MAX_LINES as f64) {
            truncated = true;
            continue;
        }
        let (Some(i_lo), Some(i_hi)) = (num::to_int::<i64>(lo_f), num::to_int::<i64>(hi_f)) else {
            truncated = true;
            continue;
        };
        let period: f64 = fam.dashes.iter().map(|d| d.abs() * k).sum();
        let mut ts: Vec<f64> = Vec::new();
        for i in i_lo..=i_hi {
            let fi = i as f64;
            let s = s0 + fi * dy;
            let t_base = o.dot(dir) + fi * dx;
            ts.clear();
            for l in loops {
                let l = l.as_ref();
                for (vi, p) in l.iter().enumerate() {
                    let q = l[(vi + 1) % l.len()];
                    let sa = p.dot(nrm) - s;
                    let sb = q.dot(nrm) - s;
                    if (sa < 0.0) != (sb < 0.0) {
                        let u = sa / (sa - sb);
                        let ta = p.dot(dir);
                        let tb = q.dot(dir);
                        ts.push(ta + (tb - ta) * u);
                    }
                }
            }
            if ts.len() < 2 {
                continue;
            }
            sort::stable(&mut ts, |a, b| a < b);
            for pair in ts.chunks_exact(2) {
                let (ta, tb) = (pair[0], pair[1]);
                if tb - ta < 1e-9 && fam.dashes.is_empty() {
                    continue;
                }
                if lines.len() >= MAX_LINES {
                    truncated = true;
                    break;
                }
                if fam.dashes.is_empty() || period < 1e-12 {
                    lines.push(line_at(dir, nrm, s, ta, tb));
                } else {
                    dash_segments(&mut lines, &fam.dashes, k, period, t_base, ta, tb, dir, nrm, s);
                }
            }
        }
    }
    HatchLines { lines, truncated }
}

#[derive(Clone, Copy, Debug)]
pub struct Grain {
    pub amp: f64,
    pub wavelength: f64,
    pub inset: f64,
}

impl Default for Grain {
    fn default() -> Grain {
        Grain { amp: 0.010, wavelength: 1.1, inset: 0.03 }
    }
}

fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn unit_hash(v_paper: f64, salt: u64) -> f64 {
    let q: i64 = num::to_int(math::round(v_paper * 1000.0)).unwrap_or(0);
    let h = mix((q as u64) ^ salt.wrapping_mul(0x2545F4914F6CDD1D));
    (h >> 11) as f64 / 9007199254740992.0
}

pub fn generate_grain<L: AsRef<[V2]>>(
    loops: &[L],
    pattern: &Pattern,
    k: f64,
    rot_deg: f64,
    grain: Grain,
) -> HatchLines {
    let base = generate(loops, pattern, k, rot_deg);
    let Some(fam0) = pattern.families.first() else { return base };
    let mut out: Vec<Line> = Vec::new();
    let th = (fam0.angle + rot_deg) * RAD_PER_DEG;
    let dir = V2::init(math::cos(th), math::sin(th));
    let nrm = V2::init(-dir.y, dir.x);
    let mut vmin = f64::INFINITY;
    let mut vmax = f64::NEG_INFINITY;
    for l in loops {
        for p in l.as_ref() {
            let v = p.dot(nrm);
            vmin = vmin.min(v);
            vmax = vmax.max(v);
        }
    }
    let tau = 2.0 * PI;
    let inset = grain.inset * k;
    let step = 0.05 * k;
    let mut truncated = base.truncated;
    for ln in &base.lines {
        let p0 = V2::init(ln[0], ln[1]);
        let p1 = V2::init(ln[2], ln[3]);
        let t0 = p0.dot(dir);
        let t1 = p1.dot(dir);
        let v = p0.dot(nrm);
        if v - vmin < inset || vmax - v < inset {
            continue;
        }
        let vp = v / k;
        let phi = tau * unit_hash(vp, 1);
        let phi2 = tau * unit_hash(vp, 2);
        let amp = grain.amp * k * (0.55 + 0.45 * unit_hash(vp, 3));
        let wl = grain.wavelength * k * (0.75 + 0.5 * unit_hash(vp, 4));
        let room = (v - vmin).min(vmax - v) - inset;
        let taper = (room / (2.0 * amp)).clamp(0.0, 1.0);
        let n: usize = num::to_int_clamped(math::ceil((t1 - t0).abs() / step), 1usize, 100_000);
        let mut prev: Option<V2> = None;
        for i in 0..=n {
            let f = i as f64 / n as f64;
            let t = t0 + (t1 - t0) * f;
            let d =
                taper * amp * (math::sin(tau * t / wl + phi) + 0.35 * math::sin(tau * t / (0.37 * wl) + phi2)) / 1.35;
            let pt = dir.scale(t).add(nrm.scale(v + d));
            let inside = |p: V2| geom::locate_even_odd(p, loops, 1e-9) == Loc::Inside;
            let ok = inside(pt) && inside(pt.add(dir.scale(inset))) && inside(pt.sub(dir.scale(inset)));
            if ok {
                if let Some(q) = prev {
                    if out.len() >= MAX_LINES {
                        truncated = true;
                        break;
                    }
                    out.push([q.x, q.y, pt.x, pt.y]);
                }
                prev = Some(pt);
            } else {
                prev = None;
            }
        }
    }
    HatchLines { lines: out, truncated }
}

fn line_at(dir: V2, nrm: V2, s: f64, ta: f64, tb: f64) -> Line {
    [dir.x * ta + nrm.x * s, dir.y * ta + nrm.y * s, dir.x * tb + nrm.x * s, dir.y * tb + nrm.y * s]
}

#[allow(clippy::too_many_arguments)]
fn dash_segments(
    lines: &mut Vec<Line>,
    dashes: &[f64],
    k: f64,
    period: f64,
    t_base: f64,
    ta: f64,
    tb: f64,
    dir: V2,
    nrm: V2,
    s: f64,
) {
    let mut m = math::floor((ta - t_base) / period);
    let m_end = math::floor((tb - t_base) / period);
    let mut guard = 0usize;
    while m <= m_end && guard < 100_000 {
        guard += 1;
        let mut t = t_base + m * period;
        for &d in dashes {
            let len = d.abs() * k;
            if d >= 0.0 {
                let a0 = t;
                let a1 = t + len;
                if len == 0.0 {
                    if a0 >= ta && a0 <= tb {
                        lines.push(line_at(dir, nrm, s, a0, a0));
                    }
                } else {
                    let c0 = a0.max(ta);
                    let c1 = a1.min(tb);
                    if c1 - c0 > 1e-9 {
                        lines.push(line_at(dir, nrm, s, c0, c1));
                    }
                }
            }
            t += len;
            if lines.len() >= MAX_LINES {
                return;
            }
        }
        m += 1.0;
    }
}
