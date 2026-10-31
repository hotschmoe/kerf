//! Section view (SPEC 8.1): exact 2D cut / beyond / hatch with crop, break lines and dedupe.

mod beyond;
mod breaks;
mod cut;
mod hardware;
mod nails;
mod occlusion;
mod strokes;

pub use cut::{FillFault, group_region};
pub use nails::NailsHidden;
pub use occlusion::Occ;
pub use strokes::{Stroke, chain_strokes, dedupe};

use crate::clip;
use crate::drawing::{FillItem, HatchItem, Item, PathItem, RegionItem};
use crate::geom::{self, Pt, V2};
use crate::model::{Kind, Prism};
use crate::pathclip;
use crate::pathgeom;
use crate::pen::Pen;
use crate::scene::Scene;
use crate::sort;
use crate::style::{Role, Style};
use crate::view::{Kind as ViewKind, ViewSpec};
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const FLAT_TOL: f64 = geom::FLAT_TOL;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Cut,
    Beyond,
    Drop,
}

/// Which stroke list a stroke is added to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Strokes,
    Ghosts,
}

/// A prism's flattened loops, shared between the section and its occluders.
pub type Flat = Rc<Vec<Vec<V2>>>;

pub struct Section<'s> {
    pub scene: &'s Scene<'s>,
    pub style: &'s Style,
    pub spec: &'s ViewSpec<'s>,
    pub prisms: Rc<[Prism]>,
    pub cls: Vec<Class>,
    /// Framing hardware drawn in front (SPEC 28.4): solid, over what it overlaps on paper.
    pub front: Vec<bool>,
    flat: Vec<Option<Flat>>,
    pub occs: Vec<Occ>,
    pub strokes: Vec<Stroke>,
    pub hatches: Vec<HatchItem>,
    pub fills: Vec<FillItem>,
    pub ghosts: Vec<Stroke>,
    pub breaks: Vec<PathItem>,
    pub seq: u32,
    pub truncated_hatch: bool,
    pub fill_faults: Vec<FillFault>,
    pub crop_loop: Vec<V2>,
    /// Where each drawn fastener of a `nailing` component is (component index, point): its notes land there.
    pub nail_marks: Vec<(u32, V2)>,
    pub nails_hidden: Vec<NailsHidden>,
}

impl<'s> Section<'s> {
    pub fn new(scene: &'s Scene<'s>, spec: &'s ViewSpec<'s>, prisms_in: &[Prism]) -> Section<'s> {
        let clipped = crate::voids::clip_fills(scene, prisms_in, spec.cut_z);
        let prisms: Rc<[Prism]> = hardware::thicken_thin_hardware(&clipped, spec.scale).into();
        let front = hardware::front_hardware(scene, spec, &prisms);
        let eps = 1e-9;
        let reaches_behind = |p: &Prism| {
            prisms
                .iter()
                .any(|q| q.lengthwise && q.comp == p.comp && q.instance == p.instance && q.z0 < spec.cut_z - eps)
        };
        let elevation = spec.kind == ViewKind::Elevation;
        let cls = prisms
            .iter()
            .zip(&front)
            .map(|(p, &f)| {
                if f || elevation || (p.lengthwise && p.z1 > spec.cut_z + eps && reaches_behind(p)) {
                    Class::Beyond
                } else if p.z0 < spec.cut_z - eps && p.z1 > spec.cut_z + eps {
                    Class::Cut
                } else if p.z1 <= spec.cut_z + eps {
                    Class::Beyond
                } else {
                    Class::Drop
                }
            })
            .collect();
        let c = spec.crop;
        let crop_loop =
            alloc::vec![V2::init(c.x0, c.y0), V2::init(c.x1, c.y0), V2::init(c.x1, c.y1), V2::init(c.x0, c.y1)];
        Section {
            scene,
            style: scene.style,
            spec,
            flat: alloc::vec![None; prisms.len()],
            prisms,
            cls,
            front,
            occs: Vec::new(),
            strokes: Vec::new(),
            hatches: Vec::new(),
            fills: Vec::new(),
            ghosts: Vec::new(),
            breaks: Vec::new(),
            seq: 0,
            truncated_hatch: false,
            fill_faults: Vec::new(),
            crop_loop,
            nail_marks: Vec::new(),
            nails_hidden: Vec::new(),
        }
    }

    /// The id of the component a prism belongs to, with `#k` for array instance k.
    pub fn src_name(&self, p: &Prism) -> String {
        let Some(c) = self.scene.comps.get(p.comp as usize) else { return String::new() };
        if c.arr_count > 1 {
            let per = c.xfs.len() / c.arr_count;
            return alloc::format!("{}#{}", c.id, (p.instance as usize).checked_div(per).unwrap_or(0));
        }
        c.id.clone()
    }

    pub fn flat_of(&mut self, i: usize) -> Flat {
        if let Some(Some(f)) = self.flat.get(i) {
            return f.clone();
        }
        let f: Flat = Rc::new(clip::from_region(&self.prisms[i].loops, FLAT_TOL));
        self.flat[i] = Some(f.clone());
        f
    }

    /// `which` if the style still defines that pen (a user style can delete one), else the beyond pen.
    pub fn pen_for(&self, which: Pen) -> Pen {
        if self.style.pen(which).is_some() { which } else { Pen::Beyond }
    }

    /// Fill materials draw with the pen of their kind (steel, rebar); every other fill material draws in the beyond pen.
    pub fn pen_for_role(&self, role: Role) -> Pen {
        match role {
            Role::Steel => self.pen_for(Pen::Steel),
            Role::Rebar => self.pen_for(Pen::Rebar),
            _ => Pen::Beyond,
        }
    }

    pub fn layer_for(&self, pen: Pen) -> String {
        self.style.layer_for_pen(pen).to_string()
    }

    pub fn add_stroke(&mut self, target: Target, pts: Vec<Pt>, closed: bool, pen: Pen, src: &str, embedded: bool) {
        let w = self.style.pen_width_mm(pen);
        self.seq += 1;
        let stroke = Stroke {
            pts,
            closed,
            pen,
            src: src.to_string(),
            rank: w + if embedded { 10.0 } else { 0.0 },
            seq: self.seq,
        };
        match target {
            Target::Strokes => self.strokes.push(stroke),
            Target::Ghosts => self.ghosts.push(stroke),
        }
    }

    /// Clip a path to the crop and add the pieces as strokes.
    pub fn add_clipped(&mut self, target: Target, pts: &[Pt], closed: bool, pen: Pen, src: &str, embedded: bool) {
        for pc in pathclip::clip_path(pts, closed, self.spec.crop) {
            self.add_stroke(target, pc.pts, pc.closed, pen, src, embedded);
        }
    }

    pub fn build(&mut self) {
        let crop = self.spec.crop;
        let margin = 1e-6;
        let prisms = Rc::clone(&self.prisms);
        for (i, p) in prisms.iter().enumerate() {
            if self.cls[i] == Class::Drop || p.kind != Kind::Body || p.face_tie || self.front[i] {
                continue;
            }
            let f = self.flat_of(i);
            let bx = clip::loops_box(&f);
            if !bx.overlaps(crop, margin) {
                continue;
            }
            self.occs.push(Occ { loops: f, bx, z1: p.z1, cut: self.cls[i] == Class::Cut, prism: i });
        }
        let holes: Vec<usize> = prisms
            .iter()
            .enumerate()
            .filter(|(i, p)| self.cls[*i] == Class::Cut && p.embedded && p.kind == Kind::Body)
            .map(|(i, _)| i)
            .collect();
        for pass in 0..2 {
            for (i, p) in prisms.iter().enumerate() {
                if self.cls[i] != Class::Cut || p.embedded != (pass == 1) {
                    continue;
                }
                self.draw_cut(i, p, &holes);
            }
        }
        for (i, p) in prisms.iter().enumerate() {
            if self.cls[i] == Class::Beyond {
                self.draw_beyond(i, p);
            }
        }
        for (i, p) in prisms.iter().enumerate() {
            if self.cls[i] == Class::Drop {
                continue;
            }
            match p.kind {
                Kind::Ghost => {
                    let pen = self.pen_for(p.pen.unwrap_or(Pen::Hidden));
                    let src = self.src_name(p);
                    for l in &p.loops {
                        self.add_clipped(Target::Ghosts, l, true, pen, &src, true);
                    }
                    if p.line_pts.len() >= 2 {
                        self.add_clipped(Target::Ghosts, &p.line_pts, false, pen, &src, true);
                    }
                }
                Kind::Line => {
                    if p.line_pts.len() >= 2 {
                        let mat_pen =
                            self.style.material(&p.material).map_or(Pen::Membrane, |m| m.pen.unwrap_or(Pen::Membrane));
                        let pen = self.pen_for(mat_pen);
                        let pts = self.line_points(p);
                        let src = self.src_name(p);
                        self.add_clipped(Target::Strokes, &pts, false, pen, &src, p.embedded);
                    }
                }
                Kind::Batt => {
                    if p.line_pts.len() >= 2 {
                        let src = self.src_name(p);
                        self.add_clipped(Target::Strokes, &p.line_pts, false, Pen::Profile, &src, false);
                    }
                }
                Kind::Body => {}
            }
            let _ = i;
        }
        self.draw_nails();
        self.break_lines();
    }

    /// The polyline a `line` prism (membrane) is drawn along. A vapor retarder keeps its dashed line visibly separate
    /// from the host edge it follows (0.03 paper inch at least).
    pub fn line_points(&self, p: &Prism) -> Vec<Pt> {
        if !(p.role == Role::VaporRetarder && p.centerline.len() >= 2 && !p.line_pts.is_empty()) {
            return p.line_pts.clone();
        }
        let c0 = p.centerline[0].v();
        let d0 = p.centerline[1].v().sub(c0).norm();
        let off = p.line_pts[0].v().sub(c0);
        let side = if d0.perp().dot(off) >= 0.0 { 1.0 } else { -1.0 };
        let gap = off.len().max(0.03 * self.spec.scale);
        pathgeom::offset_open(&p.centerline, side * gap)
    }

    /// Sawn or engineered lumber lying in the section plane (run x/y): cut along its length, not end-on.
    pub fn is_lengthwise_lumber(&self, p: &Prism) -> bool {
        if !p.quads.is_empty() || p.loops.is_empty() || p.loops[0].len() < 3 {
            return false;
        }
        self.scene.comps.get(p.comp as usize).is_some_and(|c| c.ty.traits.lengthwise_grain)
    }

    /// Non-drawn picking regions: one item per visible cut region / beyond face (outer loop + holes).
    pub fn region_items(&mut self) -> Vec<Item> {
        let mut out = Vec::new();
        let prisms = Rc::clone(&self.prisms);
        for (i, p) in prisms.iter().enumerate() {
            if self.cls[i] == Class::Drop || (p.kind == Kind::Ghost && !p.dashed) {
                continue;
            }
            let is_cut = self.cls[i] == Class::Cut && !p.dashed;
            let inst = match self.scene.comps.get(p.comp as usize) {
                Some(c) if c.arr_count > 1 => p.instance.checked_div((c.xfs.len() / c.arr_count) as u32).unwrap_or(0),
                _ => p.instance,
            };
            let part = if p.part.is_empty() { None } else { Some(p.part.clone()) };
            let src = self.src_name(p);
            if is_cut && p.kind == Kind::Body {
                let f = self.flat_of(i);
                let fb = clip::loops_box(&f);
                let c = self.spec.crop;
                if fb.x0 >= c.x0 - 1e-9 && fb.x1 <= c.x1 + 1e-9 && fb.y0 >= c.y0 - 1e-9 && fb.y1 <= c.y1 + 1e-9 {
                    out.push(Item::Region(RegionItem {
                        src,
                        part,
                        instance: inst,
                        cut: true,
                        loops: p.loops.clone(),
                        layer: String::new(),
                    }));
                    continue;
                }
            }
            let reg = self.visible_region(i);
            for g in group_region(&reg, None) {
                out.push(Item::Region(RegionItem {
                    src: src.clone(),
                    part: part.clone(),
                    instance: inst,
                    cut: is_cut,
                    loops: g.loops,
                    layer: String::new(),
                }));
            }
        }
        out
    }

    /// Items in paint order, with coincident collinear edges deduplicated.
    pub fn finish(&mut self) -> Vec<Item> {
        let mut items: Vec<Item> = Vec::new();
        items.extend(self.hatches.iter().cloned().map(Item::Hatch));
        items.extend(self.fills.iter().cloned().map(Item::Fill));
        self.knock_out_face_ties();
        let mut deduped = chain_strokes(dedupe(&self.strokes, self.style));
        sort::stable(&mut deduped, |x, y| if x.rank != y.rank { x.rank < y.rank } else { x.seq < y.seq });
        for s in deduped.into_iter().chain(self.ghosts.iter().cloned()) {
            items.push(Item::Path(PathItem {
                layer: self.layer_for(s.pen),
                pen: s.pen,
                src: s.src,
                closed: s.closed,
                pts: s.pts,
            }));
        }
        items.extend(self.breaks.iter().cloned().map(Item::Path));
        items
    }
}
