//! Compiled model: prisms, parts, anchors and the placement-resolved component list.

use crate::diag::Diag;
use crate::geom::*;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    Full,
    Top,
    None,
}

#[derive(Clone, Debug)]
pub struct ExtraStroke {
    pub pts: Vec<V>,
    pub closed: bool,
    pub pen: String,
}

/// A prism in local (builder) coordinates.
#[derive(Clone, Debug)]
pub struct LPrism {
    pub part: Option<String>,
    pub material: String,
    pub region: Region,
    pub z_off: Option<(f64, f64)>,
    pub along_z: bool,
    pub marks: Vec<[Pt; 4]>,
    pub diag_mark: bool,
    pub outline: Outline,
    pub pen: Option<String>,
    pub fill_solid: bool,
    pub extra: Vec<ExtraStroke>,
    pub bar: Option<(Pt, f64)>,
    pub embedded: Option<bool>,
    /// 3D only (iso/mesh) or 2D only (section) prisms; both by default.
    pub only: Only,
    /// For rebar paths the 3D shape is a swept circle along this centerline.
    pub sweep: Option<(Vec<Pt>, f64)>,
    /// Centerline of a thin layer (membranes) and its thickness.
    pub center: Option<(Vec<Pt>, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Only {
    Both,
    Section,
    Solid3d,
}

impl LPrism {
    pub fn new(part: Option<&str>, material: &str, region: Region) -> LPrism {
        LPrism {
            part: part.map(|s| s.to_string()),
            material: material.to_string(),
            region,
            z_off: None,
            along_z: false,
            marks: vec![],
            diag_mark: false,
            outline: Outline::Full,
            pen: None,
            fill_solid: false,
            extra: vec![],
            bar: None,
            embedded: None,
            only: Only::Both,
            sweep: None,
            center: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ZSpec {
    Run,
    Thick(f64),
}

#[derive(Clone, Debug)]
pub struct Built {
    pub prisms: Vec<LPrism>,
    pub zones: Vec<(String, Region)>,
    pub anchors: Vec<(String, Pt)>,
    pub box_: Option<Rect>,
    pub z: ZSpec,
    pub embedded: bool,
    pub material: String,
    /// Geometry is already in world coordinates (point-list based components).
    pub absolute: bool,
    pub desc: String,
}

impl Built {
    pub fn new(material: &str) -> Built {
        Built {
            prisms: vec![],
            zones: vec![],
            anchors: vec![],
            box_: None,
            z: ZSpec::Run,
            embedded: false,
            material: material.to_string(),
            absolute: false,
            desc: String::new(),
        }
    }
    pub fn compute_box(&self) -> Rect {
        if let Some(b) = self.box_ {
            return b;
        }
        let mut r = Rect::empty();
        for p in &self.prisms {
            r.union(&region_bbox(&p.region));
        }
        r
    }
    /// Apply a transform to every local datum.
    pub fn transform(&mut self, xf: &Xf) {
        for p in &mut self.prisms {
            p.region = xf.region(&p.region);
            for m in &mut p.marks {
                for q in m.iter_mut() {
                    *q = xf.apply(*q);
                }
            }
            for e in &mut p.extra {
                e.pts = xf.loop_(&e.pts);
            }
            if let Some((c, r)) = p.bar {
                p.bar = Some((xf.apply(c), r));
            }
            if let Some((pts, r)) = &p.sweep {
                p.sweep = Some((pts.iter().map(|q| xf.apply(*q)).collect(), *r));
            }
            if let Some((pts, t)) = &p.center {
                p.center = Some((pts.iter().map(|q| xf.apply(*q)).collect(), *t));
            }
        }
        for z in &mut self.zones {
            z.1 = xf.region(&z.1);
        }
        for a in &mut self.anchors {
            a.1 = xf.apply(a.1);
        }
        if let Some(b) = self.box_ {
            let mut r = Rect::empty();
            for c in b.corners() {
                r.add(xf.apply(c));
            }
            self.box_ = Some(r);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Prism {
    pub comp: usize,
    pub inst: usize,
    pub src: String,
    pub part: Option<String>,
    pub material: String,
    pub region: Region,
    pub z0: f64,
    pub z1: f64,
    pub embedded: bool,
    pub along_z: bool,
    pub marks: Vec<[Pt; 4]>,
    pub diag_mark: bool,
    pub outline: Outline,
    pub pen: Option<String>,
    pub fill_solid: bool,
    pub extra: Vec<ExtraStroke>,
    pub bar: Option<(Pt, f64)>,
    pub only: Only,
    pub sweep: Option<(Vec<Pt>, f64)>,
    pub center: Option<(Vec<Pt>, f64)>,
}

#[derive(Clone, Debug)]
pub struct PartInfo {
    pub name: String,
    pub region: Region,
    pub anchors: Vec<(String, Pt)>,
    pub zone: bool,
}

#[derive(Clone, Debug)]
pub struct Inst {
    pub index: usize,
    pub prisms: Vec<Prism>,
    pub anchors: Vec<(String, Pt)>,
    pub parts: Vec<PartInfo>,
    pub z0: f64,
    pub z1: f64,
    pub bbox: Rect,
}

#[derive(Clone, Debug)]
pub struct Comp {
    pub idx: usize,
    pub id: String,
    pub ctype: String,
    pub label: Option<String>,
    pub material: String,
    pub value: Value,
    pub insts: Vec<Inst>,
    pub array: Option<(String, usize, f64)>,
    pub embedded: bool,
    pub visible: bool,
    pub desc: String,
    pub cover: Option<Value>,
    pub failed: bool,
}

impl Comp {
    pub fn src(&self, inst: usize) -> String {
        if self.array.is_some() { format!("{}#{}", self.id, inst) } else { self.id.clone() }
    }
    pub fn bbox(&self) -> Rect {
        let mut r = Rect::empty();
        for i in &self.insts {
            r.union(&i.bbox);
        }
        r
    }
    pub fn z_range(&self) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for i in &self.insts {
            lo = lo.min(i.z0);
            hi = hi.max(i.z1);
        }
        (lo, hi)
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub id: String,
    pub title: String,
    pub run: (f64, f64),
    pub comps: Vec<Comp>,
    pub diags: Vec<Diag>,
}

impl Model {
    pub fn comp(&self, id: &str) -> Option<&Comp> {
        self.comps.iter().find(|c| c.id == id)
    }
}
