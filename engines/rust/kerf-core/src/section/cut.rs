//! Cut prisms: fills, hatching, grain, outlines, cut marks.

use super::hardware::is_path_bar;
use super::{Section, Target};
use crate::clip::{self, Op};
use crate::drawing::{FillItem, HatchItem};
use crate::geom::{self, DEG_PER_RAD, Pt, V2};
use crate::hatch;
use crate::math;
use crate::model::{Kind, OutlineMode, Prism};
use crate::pathclip;
use crate::pen::Pen;
use crate::style::CutMark;
use alloc::vec::Vec;

/// A visible region split into outer loops with their holes: exact loops (with bulges) and the flattened ones.
pub struct Group {
    pub loops: Vec<Vec<Pt>>,
    pub flat: Vec<Vec<V2>>,
}

/// Angle in degrees (0..180) of the longest edge of a loop; hatch grain follows it.
pub fn member_angle_deg(lp: &[Pt]) -> f64 {
    let mut best = -1.0;
    let mut ang = 0.0;
    for (i, p) in lp.iter().enumerate() {
        let q = lp[(i + 1) % lp.len()];
        let d = q.v().sub(p.v());
        let l = d.len();
        if l > best + 1e-9 {
            best = l;
            ang = math::atan2(d.y, d.x) * DEG_PER_RAD;
        }
    }
    ang %= 180.0;
    if ang < 0.0 {
        ang = (ang + 180.0) % 180.0;
    }
    if ang < 1e-6 || ang > 180.0 - 1e-6 {
        return 0.0;
    }
    if (ang - 90.0).abs() < 1e-6 {
        return 90.0;
    }
    ang
}

/// Splits a region into outer loops with the holes inside each; `exact` (the prism's own loops) is used as given.
pub fn group_region(region: &[Vec<V2>], exact: Option<&[Vec<Pt>]>) -> Vec<Group> {
    if let Some(ex) = exact {
        return alloc::vec![Group { loops: ex.to_vec(), flat: region.to_vec() }];
    }
    let mut groups = Vec::new();
    for outer in region.iter().filter(|l| geom::signed_area_v(l) > 0.0) {
        let mut flat: Vec<Vec<V2>> = alloc::vec![outer.clone()];
        for l in region {
            if geom::signed_area_v(l) < 0.0 && !l.is_empty() && geom::point_in_loop_eo(l[0], outer) {
                flat.push(l.clone());
            }
        }
        let loops = flat.iter().map(|l| l.iter().map(|&v| Pt::at(v, 0.0)).collect()).collect();
        groups.push(Group { loops, flat });
    }
    groups
}

impl Section<'_> {
    pub fn draw_cut(&mut self, i: usize, p: &Prism, holes: &[usize]) {
        let crop = self.spec.crop;
        let src = self.src_name(p);
        if p.kind != Kind::Body {
            return;
        }
        if p.face_tie {
            return self.draw_face_tie(i, p);
        }
        let flat = self.flat_of(i);
        let bx = clip::loops_box(&flat);
        if !bx.overlaps(crop, 1e-6) {
            return;
        }
        let path_bar = is_path_bar(p);
        let style = self.style;
        let mat = style.material(&p.material);
        let is_fill_mat = mat.is_some_and(|m| m.fill);
        let fully_inside =
            bx.x0 >= crop.x0 - 1e-9 && bx.x1 <= crop.x1 + 1e-9 && bx.y0 >= crop.y0 - 1e-9 && bx.y1 <= crop.y1 + 1e-9;
        let cut_in = style.pen_width_mm(Pen::Cut) / 25.4;
        let mut perim = 0.0;
        if let Some(f0) = flat.first() {
            for (vi, v) in f0.iter().enumerate() {
                perim += v.dist(f0[(vi + 1) % f0.len()]);
            }
        }
        let area0 = flat.first().map_or(0.0, |f0| geom::signed_area_v(f0).abs());
        let thick_model = if perim > 0.0 { 2.0 * area0 / perim } else { 1e9 };
        let is_thin = p.outline == OutlineMode::Full
            && p.role.is_metal()
            && !is_fill_mat
            && thick_model / self.spec.scale < 2.0 * cut_in;
        let pen_out = if let Some(pp) = p.pen {
            self.pen_for(pp)
        } else if is_fill_mat {
            self.pen_for_role(p.role)
        } else if is_thin {
            Pen::Steel
        } else {
            Pen::Cut
        };
        let mut region: Vec<Vec<V2>>;
        let mut region_exact = fully_inside;
        if fully_inside {
            region = flat.to_vec();
        } else {
            region = clip::boolean(&flat, &[self.crop_loop.clone()], Op::Intersect);
            region_exact = false;
        }
        if !p.embedded && !holes.is_empty() {
            let mut changed = false;
            for &h in holes {
                if h == i {
                    continue;
                }
                let hf = self.flat_of(h);
                let hb = clip::loops_box(&hf);
                if !hb.overlaps(bx, 1e-9) {
                    continue;
                }
                region = clip::boolean(&region, &hf, Op::Diff);
                changed = true;
            }
            if changed {
                region_exact = false;
            }
        }
        let hatches = mat.map_or(&[][..], |m| &m.hatch[..]);
        let want_hatch = !hatches.is_empty();
        if (want_hatch || (!path_bar && (is_fill_mat || is_thin) && p.kind == Kind::Body)) && !region.is_empty() {
            let groups = group_region(&region, region_exact.then_some(&p.loops[..]));
            for g in groups {
                if is_fill_mat || is_thin {
                    self.fills.push(FillItem {
                        layer: self.layer_for(pen_out),
                        src: src.clone(),
                        loops: g.loops.clone(),
                    });
                }
                if want_hatch && !is_thin {
                    for hs in hatches {
                        let Some(pat) = style.pattern(&hs.pattern) else { continue };
                        let res = hatch::generate(&g.flat, pat, self.spec.scale * hs.scale, hs.angle);
                        if res.truncated {
                            self.truncated_hatch = true;
                        }
                        self.hatches.push(HatchItem {
                            layer: self.layer_for(Pen::Hatch),
                            pen: Pen::Hatch,
                            src: src.clone(),
                            pattern: hs.pattern.clone(),
                            scale: hs.scale,
                            angle: hs.angle,
                            loops: g.loops.clone(),
                            lines: res.lines,
                        });
                    }
                }
            }
        }
        if let Some(gs) = mat.and_then(|m| m.grain.as_ref()) {
            if !region.is_empty() && self.is_lengthwise_lumber(p) {
                if let Some(pat) = style.pattern(&gs.pattern) {
                    let ang = member_angle_deg(&p.loops[0]);
                    let groups = group_region(&region, region_exact.then_some(&p.loops[..]));
                    for g in groups {
                        let grain =
                            hatch::Grain { amp: gs.amplitude, wavelength: gs.wavelength, ..hatch::Grain::default() };
                        let res = hatch::generate_grain(&g.flat, pat, self.spec.scale * gs.scale, ang, grain);
                        if res.truncated {
                            self.truncated_hatch = true;
                        }
                        if res.lines.is_empty() {
                            continue;
                        }
                        self.hatches.push(HatchItem {
                            layer: self.layer_for(Pen::Hatch),
                            pen: Pen::Hatch,
                            src: src.clone(),
                            pattern: gs.pattern.clone(),
                            scale: gs.scale,
                            angle: ang,
                            loops: g.loops,
                            lines: res.lines,
                        });
                    }
                }
            }
        }
        if path_bar {
            let pen = self.pen_for(Pen::Rebar);
            self.add_clipped(Target::Strokes, &p.centerline, false, pen, &src, true);
        } else {
            match p.outline {
                OutlineMode::None => {}
                OutlineMode::Full => {
                    for l in &p.loops {
                        self.add_clipped(Target::Strokes, l, true, pen_out, &src, p.embedded);
                    }
                }
                OutlineMode::Top => {
                    for l in &p.loops {
                        self.top_edges(l, pen_out, &src);
                    }
                }
            }
        }
        if let Some(m) = mat {
            let mark = if p.quads.is_empty() {
                CutMark::None
            } else if p.blocking && m.cut_mark != CutMark::None {
                CutMark::Diagonal
            } else {
                m.cut_mark
            };
            if mark != CutMark::None {
                for q in &p.quads {
                    if let Some(s) = pathclip::clip_seg(q[0], q[2], crop) {
                        self.add_stroke(
                            Target::Strokes,
                            alloc::vec![Pt::at(s[0], 0.0), Pt::at(s[1], 0.0)],
                            false,
                            Pen::Beyond,
                            &src,
                            false,
                        );
                    }
                    if mark == CutMark::X {
                        if let Some(s) = pathclip::clip_seg(q[1], q[3], crop) {
                            self.add_stroke(
                                Target::Strokes,
                                alloc::vec![Pt::at(s[0], 0.0), Pt::at(s[1], 0.0)],
                                false,
                                Pen::Beyond,
                                &src,
                                false,
                            );
                        }
                    }
                }
            }
        }
        for pl in &p.ply_lines {
            if let Some(s) = pathclip::clip_seg(pl[0], pl[1], crop) {
                self.add_stroke(
                    Target::Strokes,
                    alloc::vec![Pt::at(s[0], 0.0), Pt::at(s[1], 0.0)],
                    false,
                    Pen::Beyond,
                    &src,
                    false,
                );
            }
        }
    }
}
