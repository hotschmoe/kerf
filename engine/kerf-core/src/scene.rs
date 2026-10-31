//! The compiled scene: components (with builder output and per-instance transforms), reference
//! resolution (`comp[#k][.part]@anchor`), and zone lookups.

use crate::catalog::Entry;
use crate::geom::{self, V2, Xf};
use crate::json::Value;
use crate::model::{self, Built, Diags, Level, Prism};
use crate::style::Style;
use crate::units;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    #[default]
    Pending,
    Ok,
    Failed,
}

/// One instance of a component in a plan view: its plan box (x, -z), its parts' plan boxes and the z of its centre.
#[derive(Clone, Debug)]
pub struct PlanInst {
    pub bx: geom::Box,
    pub zc: f64,
    pub parts: Vec<(String, geom::Box)>,
}

#[derive(Clone, Debug)]
pub struct Comp<'a> {
    pub index: u32,
    pub id: String,
    pub ty: &'static Entry,
    pub node: &'a Value,
    pub label: Option<String>,
    pub state: State,
    pub built: Built,
    pub xfs: Vec<Xf>,
    pub zs: Vec<[f64; 2]>,
    pub embedded: bool,
    pub visible: bool,
    pub dashed: bool,
    /// `shown: "where_occurs"` (or the old `"dashed"`): notes that target it end in (WHERE OCCURS).
    pub where_occurs: bool,
    pub arr_count: usize,
    pub place_pt: V2,
    pub angle: f64,
    pub pitch_angle: f64,
    pub world: Vec<Prism>,
    /// What this host's voids and sleeves take away (SPEC 26.4), for W_COVER.
    pub voids: Vec<crate::voids::Cutter>,
    /// Plan views only (SPEC 26.3): per instance, where its Refs resolve in plan coordinates. Empty elsewhere.
    pub plan: Vec<PlanInst>,
}

impl<'a> Comp<'a> {
    pub fn new(index: u32, id: String, ty: &'static Entry, node: &'a Value) -> Comp<'a> {
        Comp {
            index,
            id,
            ty,
            node,
            label: None,
            state: State::Pending,
            built: Built::default(),
            xfs: Vec::new(),
            zs: Vec::new(),
            embedded: false,
            visible: true,
            dashed: false,
            where_occurs: false,
            arr_count: 1,
            place_pt: V2::init(0.0, 0.0),
            angle: 0.0,
            pitch_angle: 0.0,
            world: Vec::new(),
            voids: Vec::new(),
            plan: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefParts<'s> {
    pub comp: &'s str,
    pub inst: u32,
    pub part: Option<&'s str>,
    pub anchor: &'s str,
    pub origin: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RefError {
    BadRef,
}

fn parse_inst(s: &str) -> Option<u32> {
    match s.strip_prefix('-') {
        Some(rest) => (rest.parse::<u32>().ok()? == 0).then_some(0),
        None => s.parse::<u32>().ok(),
    }
}

pub fn parse_ref(s: &str) -> Result<RefParts<'_>, RefError> {
    let at = s.find('@').ok_or(RefError::BadRef)?;
    let head = &s[..at];
    let anchor = &s[at + 1..];
    if anchor.is_empty() {
        return Err(RefError::BadRef);
    }
    if head.is_empty() {
        if anchor == "origin" {
            return Ok(RefParts { origin: true, ..RefParts::default() });
        }
        return Err(RefError::BadRef);
    }
    let mut r = RefParts { anchor, ..RefParts::default() };
    let mut comp = head;
    if let Some(d) = head.find('.') {
        comp = &head[..d];
        let part = &head[d + 1..];
        if part.is_empty() {
            return Err(RefError::BadRef);
        }
        r.part = Some(part);
    }
    if let Some(h) = comp.find('#') {
        r.inst = parse_inst(&comp[h + 1..]).ok_or(RefError::BadRef)?;
        comp = &comp[..h];
    }
    if comp.is_empty() {
        return Err(RefError::BadRef);
    }
    r.comp = comp;
    Ok(r)
}

pub fn where_occurs_text(scene: &Scene<'_>, target: &str, text: &str) -> String {
    let mut id = target;
    if let Some(d) = id.find('.') {
        id = &id[..d];
    }
    if let Some(h) = id.find('#') {
        id = &id[..h];
    }
    let Some(c) = scene.find(id) else { return text.to_string() };
    if !c.where_occurs || text.to_ascii_lowercase().contains("where occurs") {
        return text.to_string();
    }
    format!("{} (WHERE OCCURS)", text.trim_end_matches(' '))
}

pub struct Scene<'a> {
    pub style: &'a Style,
    pub comps: Vec<Comp<'a>>,
    pub diags: &'a Diags,
    pub run: [f64; 2],
    pub default_z: Option<f64>,
    pub instances_total: usize,
}

impl<'a> Scene<'a> {
    pub fn new(style: &'a Style, diags: &'a Diags, run: [f64; 2]) -> Scene<'a> {
        Scene { style, comps: Vec::new(), diags, run, default_z: None, instances_total: 0 }
    }

    pub fn find(&self, id: &str) -> Option<&Comp<'a>> {
        self.comps.iter().find(|c| c.id == id)
    }

    pub fn comp_ids(&self) -> Vec<&str> {
        self.comps.iter().map(|c| c.id.as_str()).collect()
    }

    pub fn part_box(c: &Comp<'_>, part: &str) -> Option<geom::Box> {
        if let Some(z) = c.built.zones.iter().find(|z| z.name == part) {
            return Some(z.box_);
        }
        let mut b = geom::Box::default();
        for p in c.built.prisms.iter().filter(|p| p.part == part) {
            for l in &p.loops {
                b.add_box(geom::loop_box(l));
            }
        }
        if b.is_empty() { None } else { Some(b) }
    }

    pub fn part_names<'c>(&self, c: &'c Comp<'_>) -> Vec<&'c str> {
        let mut out: Vec<&str> = c.built.zones.iter().map(|z| z.name.as_str()).collect();
        for p in &c.built.prisms {
            if p.part.is_empty() || out.contains(&p.part.as_str()) {
                continue;
            }
            out.push(&p.part);
        }
        out
    }

    pub fn anchor_names<'c>(&self, c: &'c Comp<'_>) -> Vec<&'c str> {
        let mut out: Vec<&str> = model::BOX_ANCHOR_NAMES.to_vec();
        out.extend(c.built.anchors.iter().map(|n| n.name.as_str()));
        out
    }

    pub fn anchor_point(c: &Comp<'_>, inst: usize, part: Option<&str>, name: &str) -> Option<V2> {
        let xf = c.xfs.get(inst)?;
        if let Some(pi) = c.plan.get(inst) {
            if let Some(p) = part {
                let b = pi.parts.iter().find(|(n, _)| n == p)?.1;
                return model::box_anchor(b, name);
            }
            if let Some(v) = model::box_anchor(pi.bx, name) {
                return Some(v);
            }
            let n = c.built.anchors.iter().find(|n| n.name == name)?;
            if let Some(z) = c.built.plan_z.iter().find(|a| a.name == name) {
                return Some(V2::init(xf.apply(n.p).x, -(pi.zc + z.p.y)));
            }
            return Some(V2::init(xf.apply(n.p).x, -pi.zc));
        }
        if let Some(p) = part {
            let b = Scene::part_box(c, p)?;
            return Some(xf.apply(model::box_anchor(b, name)?));
        }
        if let Some(lp) = model::box_anchor(c.built.box_, name) {
            return Some(xf.apply(lp));
        }
        c.built.anchors.iter().find(|n| n.name == name).map(|n| xf.apply(n.p))
    }

    pub fn point(&self, v: &Value, from_id: &str, path: &str) -> Option<V2> {
        point_from_value(self, v, from_id, path)
    }

    pub fn resolve_ref_str(&self, r#ref: &str, from_id: &str, path: &str) -> Option<V2> {
        let d = self.diags;
        let Ok(r) = parse_ref(r#ref) else {
            d.add_fix(
                Level::Error,
                "E_PARAM",
                Some(from_id),
                Some(path),
                format!("'{ref}' is not a valid Ref; use \"<component>@<anchor>\", \"<component>.<part>@<anchor>\" or \"@origin\""),
                "e.g. \"sill_plate@top_left\"".to_string(),
            );
            return None;
        };
        if r.origin {
            return Some(V2::init(0.0, 0.0));
        }
        let ref_ = r#ref;
        let Some(c) = self.find(r.comp) else {
            let ids = self.comp_ids();
            if let Some(n) = model::nearest(r.comp, &ids) {
                d.add_fix(
                    Level::Error,
                    "E_REF_UNKNOWN",
                    Some(from_id),
                    Some(path),
                    format!("reference '{ref_}': no component with id '{}'; did you mean '{n}'?", r.comp),
                    n.to_string(),
                );
            } else {
                d.add(
                    Level::Error,
                    "E_REF_UNKNOWN",
                    Some(from_id),
                    Some(path),
                    format!("reference '{ref_}': no component with id '{}'. Known ids: {}", r.comp, join_ids(&ids)),
                );
            }
            return None;
        };
        if c.state != State::Ok {
            d.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(from_id),
                Some(path),
                format!("reference '{ref_}': component '{}' did not build (see its own errors), so its anchors are unavailable", r.comp),
            );
            return None;
        }
        if r.inst as usize >= c.xfs.len() {
            d.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(from_id),
                Some(path),
                format!(
                    "reference '{ref_}': component '{}' has {} instance(s); instance #{} does not exist",
                    r.comp,
                    c.xfs.len(),
                    r.inst
                ),
            );
            return None;
        }
        if let Some(p) = r.part
            && Scene::part_box(c, p).is_none()
        {
            let parts = self.part_names(c);
            let list = if parts.is_empty() { "(none)".to_string() } else { join_ids(&parts) };
            d.add(
                Level::Error,
                "E_REF_UNKNOWN",
                Some(from_id),
                Some(path),
                format!("reference '{ref_}': component '{}' has no part '{p}'. Parts: {list}", r.comp),
            );
            return None;
        }
        if let Some(pt) = Scene::anchor_point(c, r.inst as usize, r.part, r.anchor) {
            return Some(pt);
        }
        let names = self.anchor_names(c);
        d.add(
            Level::Error,
            "E_ANCHOR_UNKNOWN",
            Some(from_id),
            Some(path),
            format!(
                "reference '{ref_}': {} '{}' has no anchor '{}'. Anchors: {}",
                if r.part.is_some() { "part of" } else { "component" },
                r.comp,
                r.anchor,
                if r.part.is_some() { BOX_NAMES_TEXT.to_string() } else { join_ids(&names) },
            ),
        );
        None
    }
}

pub fn point_from_value(scene: &Scene<'_>, v: &Value, from_id: &str, path: &str) -> Option<V2> {
    match v {
        Value::String(s) => return scene.resolve_ref_str(s, from_id, path),
        Value::Array(xy) => {
            if xy.len() >= 2
                && let (Some(x), Some(y)) = (units::parse_length(&xy[0]), units::parse_length(&xy[1]))
            {
                return Some(V2::init(x, y));
            }
        }
        Value::Object(_) => {
            if let Some(rs) = v.get("ref").and_then(Value::str) {
                let mut pt = scene.resolve_ref_str(rs, from_id, path)?;
                if let Some(off) = v.get("offset").filter(|o| !o.is_null()) {
                    let opath = format!("{path}/offset");
                    pt = pt.add(model::offset_pair_or_diag(scene.diags, Some(from_id), &opath, "offset", off)?);
                }
                return Some(pt);
            }
        }
        _ => {}
    }
    scene.diags.add(
        Level::Error,
        "E_PARAM",
        Some(from_id),
        Some(path),
        "expected a point: a Ref \"comp@anchor\", {\"ref\": ..., \"offset\": [dx, dy]} or [x, y]".to_string(),
    );
    None
}

const BOX_NAMES_TEXT: &str =
    "top_left, top_center, top_right, middle_left, center, middle_right, bottom_left, bottom_center, bottom_right";

pub fn join_ids<S: AsRef<str>>(ids: &[S]) -> String {
    let mut out = String::new();
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(id.as_ref());
    }
    out
}
