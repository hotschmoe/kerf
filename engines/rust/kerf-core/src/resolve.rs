//! Reference parsing, placement DAG resolution, and the `compile` entry that produces a `Model`.

use crate::build::{self, BuildIn, Host};
use crate::diag::{Diag, nearest};
use crate::geom::*;
use crate::model::*;
use crate::num::{fmt_ftin, parse_slope};
use crate::schema;
use crate::style::Style;
use serde_json::{Map, Value};

#[derive(Clone, Debug)]
pub struct RefP {
    pub comp: Option<String>,
    pub inst: usize,
    pub part: Option<String>,
    pub anchor: String,
}

pub fn parse_ref(s: &str) -> Result<RefP, String> {
    let s = s.trim();
    let Some((left, anchor)) = s.split_once('@') else {
        return Err(format!("reference {:?} must look like \"component@anchor\", \"component.part@anchor\" or \"@origin\"", s));
    };
    if left.is_empty() {
        if anchor == "origin" {
            return Ok(RefP { comp: None, inst: 0, part: None, anchor: "origin".into() });
        }
        return Err(format!("reference {:?}: only \"@origin\" may omit the component", s));
    }
    if anchor.is_empty() {
        return Err(format!("reference {:?} is missing the anchor name after '@'", s));
    }
    let end = left.find(['#', '.']).unwrap_or(left.len());
    let id = &left[..end];
    let mut rest = &left[end..];
    let mut inst = 0usize;
    let mut part: Option<String> = None;
    if let Some(r) = rest.strip_prefix('#') {
        let e = r.find('.').unwrap_or(r.len());
        inst = r[..e].parse::<usize>().map_err(|_| format!("reference {:?}: instance index after '#' must be a number", s))?;
        rest = &r[e..];
    }
    if let Some(r) = rest.strip_prefix('.') {
        part = Some(r.to_string());
        rest = "";
    }
    if !rest.is_empty() {
        return Err(format!("reference {:?} is malformed", s));
    }
    Ok(RefP { comp: Some(id.to_string()), inst, part, anchor: anchor.to_string() })
}

/// Component id mentioned by a reference string (for dependency ordering).
fn ref_comp(s: &str) -> Option<String> {
    parse_ref(s).ok().and_then(|r| r.comp)
}

fn push_value_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            if let Some(c) = ref_comp(s) {
                out.push(c);
            }
        }
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("ref") {
                if let Some(c) = ref_comp(s) {
                    out.push(c);
                }
            }
        }
        _ => {}
    }
}

/// Component ids a component's resolution depends on.
pub fn dependencies(c: &Map<String, Value>) -> Vec<String> {
    let mut out = vec![];
    if let Some(at) = c.get("at").and_then(|a| a.as_object()) {
        if let Some(to) = at.get("to") {
            push_value_refs(to, &mut out);
        }
    }
    if let Some(Value::Array(pts)) = c.get("points") {
        for p in pts {
            push_value_refs(p, &mut out);
        }
    }
    if let Some(Value::Object(pr)) = c.get("profile") {
        if let Some(Value::Array(pts)) = pr.get("points") {
            for p in pts {
                push_value_refs(p, &mut out);
            }
        }
    }
    if let Some(Value::Object(pl)) = c.get("place") {
        if let Some(Value::String(s)) = pl.get("in") {
            let end = s.find(['#', '.']).unwrap_or(s.len());
            out.push(s[..end].to_string());
        }
    }
    out
}

pub struct Lookup<'a> {
    pub comps: &'a [Comp],
    /// Every component id in the document (for "did you mean" hints before all are resolved).
    pub all_ids: &'a [String],
}

impl<'a> Lookup<'a> {
    fn find(&self, id: &str) -> Option<&'a Comp> {
        self.comps.iter().find(|c| c.id == id)
    }
    fn ids(&self) -> Vec<&'a str> {
        if self.all_ids.is_empty() { self.comps.iter().map(|c| c.id.as_str()).collect() } else { self.all_ids.iter().map(|s| s.as_str()).collect() }
    }

    /// Resolve a Ref to a world point.
    pub fn point(&self, r: &RefP) -> Result<Pt, Diag> {
        let Some(cid) = &r.comp else {
            return Ok(pt(0.0, 0.0));
        };
        let Some(c) = self.find(cid) else {
            let ids = self.ids();
            let mut d = Diag::error("E_REF_UNKNOWN", format!("reference to unknown component \"{}\".", cid));
            if let Some(n) = nearest(cid, ids.iter().copied()) {
                d = d.fix(format!("did you mean \"{}\"? Existing components: {}", n, ids.join(", ")));
            } else {
                d = d.fix(format!("existing components: {}", ids.join(", ")));
            }
            return Err(d);
        };
        if c.failed || c.insts.is_empty() {
            return Err(Diag::error("E_REF_UNKNOWN", format!("component \"{}\" could not be built (fix its errors first), so \"{}@{}\" is unavailable.", cid, cid, r.anchor)));
        }
        let Some(inst) = c.insts.get(r.inst) else {
            return Err(Diag::error("E_REF_UNKNOWN", format!("component \"{}\" has {} instance(s); instance #{} does not exist.", cid, c.insts.len(), r.inst)));
        };
        if let Some(part) = &r.part {
            let Some(pi) = inst.parts.iter().find(|p| &p.name == part) else {
                let names: Vec<&str> = inst.parts.iter().map(|p| p.name.as_str()).collect();
                let mut d = Diag::error("E_REF_UNKNOWN", format!("component \"{}\" has no part \"{}\".", cid, part));
                d = d.fix(if names.is_empty() {
                    format!("\"{}\" has no named parts; reference \"{}@<anchor>\" instead", cid, cid)
                } else {
                    format!("parts of {}: {}", cid, names.join(", "))
                });
                return Err(d);
            };
            return pi.anchors.iter().find(|a| a.0 == r.anchor).map(|a| a.1).ok_or_else(|| {
                Diag::error("E_ANCHOR_UNKNOWN", format!("part \"{}.{}\" has no anchor \"{}\".", cid, part, r.anchor))
                    .fix(format!("available: {}", pi.anchors.iter().map(|a| a.0.as_str()).collect::<Vec<_>>().join(", ")))
            });
        }
        inst.anchors.iter().find(|a| a.0 == r.anchor).map(|a| a.1).ok_or_else(|| {
            let names: Vec<&str> = inst.anchors.iter().map(|a| a.0.as_str()).collect();
            let mut d = Diag::error("E_ANCHOR_UNKNOWN", format!("component \"{}\" has no anchor \"{}\".", cid, r.anchor));
            let hint = nearest(&r.anchor, names.iter().copied()).map(|n| format!("did you mean \"{}\"? ", n)).unwrap_or_default();
            d = d.fix(format!("{}available anchors: {}", hint, names.join(", ")));
            d
        })
    }

    pub fn point_str(&self, s: &str) -> Result<Pt, Diag> {
        let r = parse_ref(s).map_err(|e| Diag::error("E_PARAM", e))?;
        self.point(&r)
    }

    /// A Ref string, {ref, offset}, or literal [x, y].
    pub fn point_value(&self, v: &Value) -> Result<Pt, Diag> {
        match v {
            Value::String(s) => self.point_str(s),
            Value::Object(m) => {
                let base = match m.get("ref").and_then(|r| r.as_str()) {
                    Some(s) => self.point_str(s)?,
                    None => return Err(Diag::error("E_PARAM", "point object needs a \"ref\" string")),
                };
                let off = m.get("offset").and_then(lit_pt).unwrap_or(pt(0.0, 0.0));
                Ok(base + off)
            }
            Value::Array(_) => lit_pt(v).ok_or_else(|| Diag::error("E_PARAM", "literal point must be [x, y] numbers (inches)")),
            _ => Err(Diag::error("E_PARAM", "expected a Ref string, {ref, offset} or [x, y]")),
        }
    }
}

pub fn lit_pt(v: &Value) -> Option<Pt> {
    let a = v.as_array()?;
    if a.len() < 2 {
        return None;
    }
    Some(pt(a[0].as_f64()?, a[1].as_f64()?))
}

fn comp_ids(doc: &Value) -> Vec<String> {
    doc.get("components")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|c| c.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

fn fail(diags: &mut Vec<Diag>, mut ds: Vec<Diag>, id: &str) {
    for d in ds.drain(..) {
        diags.push(if d.id.is_none() { d.id(id) } else { d });
    }
}

fn with_id(d: Diag, id: &str, path: &str) -> Diag {
    let mut d = d.id(id);
    if d.path.is_none() {
        d = d.path(path.to_string());
    }
    d
}

/// Compile a canonical document value into a placed model. Never panics on bad input.
pub fn compile(doc: &Value, style: &Style, diags: &mut Vec<Diag>) -> Model {
    let id = doc.get("id").and_then(|v| v.as_str()).unwrap_or("untitled").to_string();
    let title = doc.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let run = match doc.get("run").and_then(|r| r.as_array()) {
        Some(a) if a.len() == 2 => (a[0].as_f64().unwrap_or(-24.0), a[1].as_f64().unwrap_or(24.0)),
        _ => (-24.0, 24.0),
    };
    let comps_v: Vec<Value> = doc.get("components").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let ids = comp_ids(doc);

    // ids: pattern and duplicates
    let mut seen: Vec<&str> = vec![];
    let mut dup = vec![false; comps_v.len()];
    for (k, c) in comps_v.iter().enumerate() {
        let cid = c.get("id").and_then(|i| i.as_str()).unwrap_or("");
        if cid.is_empty() {
            diags.push(Diag::error("E_PARAM", format!("components[{}]: missing \"id\" (use [a-z][a-z0-9_]*, e.g. \"sill_plate\")", k)).path(format!("components/{}", k)));
            dup[k] = true;
            continue;
        }
        if !schema::valid_id(cid) {
            diags.push(Diag::error("E_PARAM", format!("component id \"{}\" is invalid: ids are [a-z][a-z0-9_]* (lowercase, digits, underscore).", cid)).id(cid).path(format!("components/{}/id", cid)));
        }
        if seen.contains(&cid) {
            diags.push(Diag::error("E_DUP_ID", format!("duplicate component id \"{}\" (component #{}). Ids must be unique across all components.", cid, k + 1)).id(cid).fix("rename one of them"));
            dup[k] = true;
        }
        seen.push(cid);
    }

    // dependency order
    let n = comps_v.len();
    let deps: Vec<Vec<usize>> = comps_v
        .iter()
        .map(|c| {
            let m = c.as_object();
            let mut d: Vec<usize> = vec![];
            if let Some(m) = m {
                for dep in dependencies(m) {
                    if let Some(pos) = ids.iter().position(|x| *x == dep) {
                        d.push(pos);
                    }
                }
            }
            d
        })
        .collect();
    let mut done = vec![false; n];
    let mut order: Vec<usize> = vec![];
    loop {
        let mut progressed = false;
        for k in 0..n {
            if done[k] {
                continue;
            }
            if deps[k].iter().all(|&d| done[d]) {
                done[k] = true;
                order.push(k);
                progressed = true;
                break;
            }
        }
        if !progressed {
            break;
        }
    }
    let cyc: Vec<usize> = (0..n).filter(|k| !done[*k]).collect();
    if !cyc.is_empty() {
        // find one cycle by walking dependencies
        let mut path: Vec<usize> = vec![];
        let mut cur = cyc[0];
        loop {
            if let Some(p) = path.iter().position(|&x| x == cur) {
                path.drain(..p);
                break;
            }
            path.push(cur);
            match deps[cur].iter().find(|d| cyc.contains(d)) {
                Some(&nx) => cur = nx,
                None => break,
            }
        }
        let names: Vec<String> = path.iter().map(|&k| ids[k].clone()).collect();
        let mut loop_names = names.clone();
        if let Some(f) = names.first() {
            loop_names.push(f.clone());
        }
        diags.push(
            Diag::error("E_CYCLE", format!("placement cycle: {}. Each component's position must derive from components that do not depend on it.", loop_names.join(" -> ")))
                .id(names.first().cloned().unwrap_or_default())
                .fix(format!("give \"{}\" an absolute position (at.to = [x, y]) or reference a different component", names.first().cloned().unwrap_or_default())),
        );
        for &k in &cyc {
            order.push(k); // resolved with whatever exists; unresolved refs will report E_REF_UNKNOWN
        }
    }

    let mut comps: Vec<Comp> = vec![];
    for &k in &order {
        let cv = &comps_v[k];
        let cid = ids.get(k).cloned().unwrap_or_default();
        if dup[k] {
            continue;
        }
        let comp = resolve_comp(k, cv, &cid, style, run, &comps, &ids, diags);
        comps.push(comp);
    }
    comps.sort_by_key(|c| c.idx);
    Model { id, title, run, comps, diags: vec![] }
}

fn at_parts(c: &Map<String, Value>) -> (Option<&Map<String, Value>>, String) {
    let at = c.get("at").and_then(|a| a.as_object());
    let anchor = at.and_then(|a| a.get("anchor")).and_then(|a| a.as_str()).unwrap_or("bottom_left").to_string();
    (at, anchor)
}

fn uses_points(ctype: &str, m: &Map<String, Value>) -> bool {
    match ctype {
        "connector" | "membrane" | "fill" => true,
        "rebar" => m.get("mode").and_then(|v| v.as_str()) == Some("path"),
        "concrete" => m.get("shape").and_then(|v| v.as_str()) == Some("polygon"),
        "insulation" => m.get("points").map_or(false, |v| !v.is_null()),
        "solid" => m.get("profile").and_then(|p| p.get("points")).is_some(),
        _ => false,
    }
}

fn resolve_comp(idx: usize, cv: &Value, cid: &str, style: &Style, run: (f64, f64), resolved: &[Comp], all_ids: &[String], diags: &mut Vec<Diag>) -> Comp {
    let empty = Map::new();
    let m = cv.as_object().unwrap_or(&empty);
    let ctype = m.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let mut comp = Comp {
        idx,
        id: cid.to_string(),
        ctype: ctype.clone(),
        label: m.get("label").and_then(|l| l.as_str()).map(|s| s.to_string()),
        material: m.get("material").and_then(|l| l.as_str()).unwrap_or("").to_string(),
        value: cv.clone(),
        insts: vec![],
        array: None,
        embedded: false,
        visible: m.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        desc: String::new(),
        cover: None,
        failed: true,
    };
    if schema::type_spec(&ctype).is_none() {
        return comp; // E_PARAM already reported by canonicalization
    }
    let lk = Lookup { comps: resolved, all_ids };
    let cpath = format!("components/{}", cid);

    // placement
    let (at, anchor_name) = at_parts(m);
    let mut place_pt: Option<Pt> = None;
    if let Some(at) = at {
        if let Some(to) = at.get("to") {
            match lk.point_value(to) {
                Ok(p) => place_pt = Some(p),
                Err(d) => {
                    diags.push(with_id(d, cid, &format!("{}/at/to", cpath)));
                    return comp;
                }
            }
        }
        let off = at.get("offset").and_then(lit_pt).unwrap_or(pt(0.0, 0.0));
        place_pt = Some(place_pt.unwrap_or(pt(0.0, 0.0)) + off);
    }

    // point list
    let mut pts: Option<Vec<V>> = None;
    if uses_points(&ctype, m) {
        let list = if ctype == "solid" { m.get("profile").and_then(|p| p.get("points")) } else { m.get("points") };
        match list.and_then(|l| l.as_array()) {
            Some(arr) => {
                let mut out = vec![];
                let mut any_ref = false;
                for e in arr {
                    if matches!(e, Value::String(_) | Value::Object(_)) {
                        any_ref = true;
                    }
                }
                let _ = any_ref;
                for (k, e) in arr.iter().enumerate() {
                    match e {
                        Value::Array(a) => {
                            let x = a.first().and_then(|x| x.as_f64()).unwrap_or(0.0);
                            let y = a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0);
                            let b = a.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0);
                            let off = place_pt.unwrap_or(pt(0.0, 0.0));
                            out.push(vb(x + off.x, y + off.y, b));
                        }
                        other => match lk.point_value(other) {
                            Ok(p) => out.push(v(p.x, p.y)),
                            Err(d) => {
                                diags.push(with_id(d, cid, &format!("{}/points/{}", cpath, k)));
                                return comp;
                            }
                        },
                    }
                }
                pts = Some(out);
            }
            None => {
                diags.push(Diag::error("E_PARAM", format!("{}/points: required (array of points)", cpath)).id(cid).path(format!("{}/points", cpath)));
                return comp;
            }
        }
    }

    // rebar host
    let mut host: Option<Host> = None;
    if ctype == "rebar" && m.get("place").map_or(false, |p| !p.is_null()) {
        let pl = m.get("place").and_then(|p| p.as_object());
        let spec = pl.and_then(|p| p.get("in")).and_then(|p| p.as_str()).unwrap_or("");
        if spec.is_empty() {
            diags.push(Diag::error("E_PARAM", format!("{}/place/in: required, a component or \"component.part\" holding the bars", cpath)).id(cid).path(format!("{}/place/in", cpath)));
            return comp;
        }
        let (hid, hpart) = match spec.split_once('.') {
            Some((a, b)) => (a, Some(b)),
            None => (spec, None),
        };
        let Some(hc) = resolved.iter().find(|c| c.id == hid) else {
            let ids: Vec<&str> = resolved.iter().map(|c| c.id.as_str()).collect();
            let mut d = Diag::error("E_REF_UNKNOWN", format!("{}/place/in: unknown host component \"{}\".", cpath, hid)).id(cid).path(format!("{}/place/in", cpath));
            if let Some(nn) = nearest(hid, ids.iter().copied()) {
                d = d.fix(format!("did you mean \"{}\"?", nn));
            }
            diags.push(d);
            return comp;
        };
        let Some(hinst) = hc.insts.first() else {
            diags.push(Diag::error("E_REF_UNKNOWN", format!("{}/place/in: host \"{}\" could not be built", cpath, hid)).id(cid));
            return comp;
        };
        let region = match hpart {
            Some(pn) => match hinst.parts.iter().find(|p| p.name == pn) {
                Some(p) => p.region.clone(),
                None => {
                    let names: Vec<&str> = hinst.parts.iter().map(|p| p.name.as_str()).collect();
                    diags.push(
                        Diag::error("E_REF_UNKNOWN", format!("{}/place/in: host \"{}\" has no part \"{}\".", cpath, hid, pn))
                            .id(cid)
                            .path(format!("{}/place/in", cpath))
                            .fix(format!("parts of {}: {}", hid, if names.is_empty() { "(none)".to_string() } else { names.join(", ") })),
                    );
                    return comp;
                }
            },
            None => Region::new(hinst.bbox.loop_()),
        };
        host = Some(Host { region });
    }

    let inp = BuildIn { id: cid, ctype: &ctype, m, style, run, pts, host };
    let mut built = match build::build(&inp) {
        Ok(b) => b,
        Err(ds) => {
            fail(diags, ds, cid);
            return comp;
        }
    };

    // local frame
    let mirror = m.get("mirror").and_then(|v| v.as_bool()).unwrap_or(false);
    if mirror {
        let bx = built.compute_box();
        built.transform(&Xf::mirror_x(bx.cx()));
    }
    let bx = built.compute_box();
    let mut local_anchors: Vec<(String, Pt)> = vec![];
    for name in BOX_ANCHORS {
        if let Some(p) = bx.anchor(name) {
            local_anchors.push((name.to_string(), p));
        }
    }
    for (n, p) in &built.anchors {
        local_anchors.retain(|a| &a.0 != n);
        local_anchors.push((n.clone(), *p));
    }

    // rotation / slope
    let mut ang = m.get("rotate").and_then(|r| r.as_f64()).unwrap_or(0.0);
    if let Some(sv) = m.get("slope") {
        if !sv.is_null() {
            match parse_slope(sv) {
                Ok(d) => ang += d,
                Err(e) => {
                    diags.push(Diag::error("E_PARAM", format!("{}/slope: {}", cpath, e)).id(cid).path(format!("{}/slope", cpath)));
                    return comp;
                }
            }
        }
    }
    let rad = ang.to_radians();

    let xf = if built.absolute {
        match place_pt {
            Some(p) if ang != 0.0 => Xf::translate(p.x, p.y).after(&Xf::rotate(rad)).after(&Xf::translate(-p.x, -p.y)),
            _ => Xf::identity(),
        }
    } else {
        let a = match local_anchors.iter().find(|a| a.0 == anchor_name) {
            Some(a) => a.1,
            None => {
                let names: Vec<&str> = local_anchors.iter().map(|a| a.0.as_str()).collect();
                let mut d = Diag::error("E_ANCHOR_UNKNOWN", format!("{}/at/anchor: \"{}\" is not an anchor of this {}.", cpath, anchor_name, ctype)).id(cid).path(format!("{}/at/anchor", cpath));
                let hint = nearest(&anchor_name, names.iter().copied()).map(|n| format!("did you mean \"{}\"? ", n)).unwrap_or_default();
                d = d.fix(format!("{}available anchors: {}", hint, names.join(", ")));
                diags.push(d);
                return comp;
            }
        };
        let p = place_pt.unwrap_or(pt(0.0, 0.0));
        Xf::translate(p.x, p.y).after(&Xf::rotate(rad)).after(&Xf::translate(-a.x, -a.y))
    };

    // z extent
    let (zr0, zr1) = run;
    let (mut z0, mut z1) = match built.z {
        ZSpec::Run => (zr0, zr1),
        ZSpec::Thick(t) => {
            let mid = (zr0 + zr1) * 0.5;
            (mid - t * 0.5, mid + t * 0.5)
        }
    };
    if let Some(zv) = m.get("z") {
        match zv {
            Value::Array(a) if a.len() == 2 => {
                let (a0, a1) = (a[0].as_f64().unwrap_or(z0), a[1].as_f64().unwrap_or(z1));
                z0 = a0.min(a1);
                z1 = a0.max(a1);
            }
            Value::Number(nv) => match built.z {
                ZSpec::Thick(t) => {
                    let c = nv.as_f64().unwrap_or(0.0);
                    z0 = c - t * 0.5;
                    z1 = c + t * 0.5;
                }
                ZSpec::Run => {
                    diags.push(
                        Diag::error("E_PARAM", format!("{}/z: this {} runs along Z, so it has no natural thickness to center; give z as [z0, z1]", cpath, ctype))
                            .id(cid)
                            .path(format!("{}/z", cpath))
                            .fix(format!("set \"z\": [{}, {}]", crate::num::fmt_num(zr0), crate::num::fmt_num(zr1))),
                    );
                    return comp;
                }
            },
            _ => {}
        }
    }

    // array
    let mut array: Option<(String, usize, f64)> = None;
    if let Some(Value::Object(am)) = m.get("array") {
        let axis = am.get("axis").and_then(|a| a.as_str()).unwrap_or("z").to_string();
        let count = am.get("count").and_then(|c| c.as_f64()).unwrap_or(1.0).max(1.0) as usize;
        if count > 500 {
            diags.push(Diag::error("E_PARAM", format!("{}/array/count: {} is too many instances (limit 500)", cpath, count)).id(cid).path(format!("{}/array/count", cpath)));
            return comp;
        }
        let spacing = am.get("spacing").and_then(|c| c.as_f64()).unwrap_or(0.0);
        array = Some((axis, count, spacing));
    }
    let ninst = array.as_ref().map(|a| a.1).unwrap_or(1);

    // zone / part sets (local), shared by instances
    let mut part_names: Vec<(String, bool)> = vec![];
    for (zn, _) in &built.zones {
        part_names.push((zn.clone(), true));
    }
    for p in &built.prisms {
        if let Some(pn) = &p.part {
            if !part_names.iter().any(|x| &x.0 == pn) {
                part_names.push((pn.clone(), false));
            }
        }
    }

    let embedded = m.get("embedded").and_then(|v| v.as_bool()).unwrap_or(built.embedded);
    let ca = comp_cover(&ctype, m);
    for k in 0..ninst {
        let (dx, dy, dz) = match &array {
            Some((axis, _, sp)) => match axis.as_str() {
                "x" => (sp * k as f64, 0.0, 0.0),
                "y" => (0.0, sp * k as f64, 0.0),
                _ => (0.0, 0.0, sp * k as f64),
            },
            None => (0.0, 0.0, 0.0),
        };
        let ixf = Xf::translate(dx, dy).after(&xf);
        let mut prisms = vec![];
        let mut bb = Rect::empty();
        for lp in &built.prisms {
            let region = ixf.region(&lp.region);
            bb.union(&region_bbox(&region));
            let (pz0, pz1) = match lp.z_off {
                Some((a, b)) => (a + dz, b + dz),
                None => (z0 + dz, z1 + dz),
            };
            prisms.push(Prism {
                comp: idx,
                inst: k,
                src: if array.is_some() { format!("{}#{}", cid, k) } else { cid.to_string() },
                part: lp.part.clone(),
                material: lp.material.clone(),
                region,
                z0: pz0,
                z1: pz1,
                embedded: lp.embedded.unwrap_or(embedded),
                along_z: lp.along_z,
                marks: lp.marks.iter().map(|q| [ixf.apply(q[0]), ixf.apply(q[1]), ixf.apply(q[2]), ixf.apply(q[3])]).collect(),
                diag_mark: lp.diag_mark,
                outline: lp.outline,
                pen: lp.pen.clone(),
                fill_solid: lp.fill_solid,
                extra: lp.extra.iter().map(|e| ExtraStroke { pts: ixf.loop_(&e.pts), closed: e.closed, pen: e.pen.clone() }).collect(),
                bar: lp.bar.map(|(c, r)| (ixf.apply(c), r)),
                only: lp.only,
                sweep: lp.sweep.as_ref().map(|(p, r)| (p.iter().map(|q| ixf.apply(*q)).collect(), *r)),
                center: lp.center.as_ref().map(|(p, t)| (p.iter().map(|q| ixf.apply(*q)).collect(), *t)),
            });
        }
        // anchors
        let anchors: Vec<(String, Pt)> = local_anchors.iter().map(|(n, p)| (n.clone(), ixf.apply(*p))).collect();
        // parts
        let mut parts = vec![];
        for (pn, is_zone) in &part_names {
            let mut lregions: Vec<&Region> = vec![];
            if *is_zone {
                if let Some(z) = built.zones.iter().find(|z| &z.0 == pn) {
                    lregions.push(&z.1);
                }
            } else {
                for lp in built.prisms.iter().filter(|p| p.part.as_ref() == Some(pn)) {
                    lregions.push(&lp.region);
                }
            }
            let mut lb = Rect::empty();
            for r in &lregions {
                lb.union(&region_bbox(r));
            }
            let mut pa = vec![];
            for name in BOX_ANCHORS {
                if let Some(p) = lb.anchor(name) {
                    pa.push((name.to_string(), ixf.apply(p)));
                }
            }
            let region = if lregions.len() == 1 {
                ixf.region(lregions[0])
            } else {
                // multiple prisms: box region of the union (zone semantics)
                let mut bx = Rect::empty();
                for r in &lregions {
                    bx.union(&region_bbox(r));
                }
                ixf.region(&Region::new(bx.loop_()))
            };
            parts.push(PartInfo { name: pn.clone(), region, anchors: pa, zone: *is_zone });
        }
        comp.insts.push(Inst { index: k, prisms, anchors, parts, z0: z0 + dz, z1: z1 + dz, bbox: bb });
    }
    comp.array = array;
    comp.embedded = embedded;
    if comp.material.is_empty() {
        comp.material = built.material.clone();
    }
    comp.desc = built.desc.clone();
    comp.cover = ca;
    comp.failed = false;
    comp
}

fn comp_cover(ctype: &str, m: &Map<String, Value>) -> Option<Value> {
    let shape = m.get("shape").and_then(|s| s.as_str()).unwrap_or("");
    let mut base = build::cover_default(ctype, shape)?;
    if let (Some(Value::Object(user)), Some(bm)) = (m.get("cover"), base.as_object_mut()) {
        for (k, v) in user {
            bm.insert(k.clone(), v.clone());
        }
    }
    Some(base)
}

pub fn fmt_pt(p: Pt) -> String {
    format!("({}, {})", fmt_ftin(p.x), fmt_ftin(p.y))
}
