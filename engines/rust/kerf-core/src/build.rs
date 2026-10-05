//! Component builders: canonical component object -> local prisms, zones and anchors (SPEC section 5).

use crate::diag::Diag;
use crate::geom::*;
use crate::model::*;
use crate::num::{fmt_ftin, fmt_num, length_of, parse_slope};
use crate::stroke::*;
use crate::style::Style;
use serde_json::{Map, Value};

pub struct Host {
    pub region: Region,
}

pub struct BuildIn<'a> {
    pub id: &'a str,
    pub ctype: &'a str,
    pub m: &'a Map<String, Value>,
    pub style: &'a Style,
    pub run: (f64, f64),
    /// Resolved world points for point-list components.
    pub pts: Option<Vec<V>>,
    pub host: Option<Host>,
}

type R = Result<Built, Vec<Diag>>;

impl<'a> BuildIn<'a> {
    fn f(&self, k: &str, d: f64) -> f64 {
        self.m.get(k).and_then(|v| v.as_f64()).unwrap_or(d)
    }
    fn s(&self, k: &str, d: &str) -> String {
        self.m.get(k).and_then(|v| v.as_str()).unwrap_or(d).to_string()
    }
    fn b(&self, k: &str, d: bool) -> bool {
        self.m.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
    }
    fn has(&self, k: &str) -> bool {
        self.m.get(k).map_or(false, |v| !v.is_null())
    }
    fn err(&self, field: &str, msg: String) -> Vec<Diag> {
        vec![Diag::error("E_PARAM", msg).id(self.id).path(format!("components/{}/{}", self.id, field))]
    }
    fn material(&self, default: &str) -> String {
        self.s("material", default)
    }
    fn need_pts(&self, what: &str) -> Result<Vec<V>, Vec<Diag>> {
        match &self.pts {
            Some(p) if p.len() >= 2 => Ok(p.clone()),
            _ => Err(self.err("points", format!("components/{}/points: {} needs at least 2 points", self.id, what))),
        }
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
    Region::new(Rect::new(x0, y0, x1, y1).loop_())
}

fn quad(x0: f64, y0: f64, x1: f64, y1: f64) -> [Pt; 4] {
    [pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)]
}

pub fn rebar_dia(size: &str) -> Option<f64> {
    match size {
        "#3" => Some(0.375),
        "#4" => Some(0.5),
        "#5" => Some(0.625),
        "#6" => Some(0.75),
        "#7" => Some(0.875),
        "#8" => Some(1.0),
        _ => None,
    }
}

pub fn gauge_t(g: i64) -> Option<f64> {
    match g {
        10 => Some(0.1345),
        12 => Some(0.1046),
        14 => Some(0.0747),
        16 => Some(0.0598),
        18 => Some(0.0478),
        20 => Some(0.0359),
        _ => None,
    }
}

/// model -> (width, gauge, length, kind)
pub fn hardware(model: &str) -> Option<(f64, i64, f64, &'static str)> {
    let m = model.to_uppercase();
    let t: &[(&str, f64, i64, f64, &str)] = &[
        ("H2.5A", 1.375, 18, 5.5, "hurricane tie"),
        ("H1", 1.375, 18, 5.5, "hurricane tie"),
        ("H10A", 1.5, 18, 8.0, "hurricane tie"),
        ("MSTA9", 1.25, 12, 9.0, "strap"),
        ("MSTA12", 1.25, 12, 12.0, "strap"),
        ("MSTA15", 1.25, 12, 15.0, "strap"),
        ("MSTA18", 1.25, 12, 18.0, "strap"),
        ("MSTA21", 1.25, 12, 21.0, "strap"),
        ("MSTA24", 1.25, 12, 24.0, "strap"),
        ("MSTA30", 1.25, 12, 30.0, "strap"),
        ("MSTA36", 1.25, 12, 36.0, "strap"),
        ("MSTA49", 1.25, 12, 49.0, "strap"),
        ("LSTA9", 1.25, 20, 9.0, "strap"),
        ("LSTA12", 1.25, 20, 12.0, "strap"),
        ("LSTA15", 1.25, 20, 15.0, "strap"),
        ("LSTA18", 1.25, 20, 18.0, "strap"),
        ("LSTA21", 1.25, 20, 21.0, "strap"),
        ("LSTA24", 1.25, 20, 24.0, "strap"),
        ("LSTA36", 1.25, 20, 36.0, "strap"),
        ("CS14", 1.25, 14, 0.0, "coil strap"),
        ("CS16", 1.25, 16, 0.0, "coil strap"),
        ("CS18", 1.25, 18, 0.0, "coil strap"),
        ("CS20", 1.25, 20, 0.0, "coil strap"),
        ("CMST14", 3.0, 14, 0.0, "coil strap"),
        ("CMST12", 3.0, 12, 0.0, "coil strap"),
        ("CMSTC16", 3.0, 16, 0.0, "coil strap"),
        ("MST37", 3.0, 12, 37.5, "strap"),
        ("MST48", 3.0, 12, 48.0, "strap"),
        ("META12", 1.25, 18, 12.0, "embedded truss anchor"),
        ("META16", 1.25, 18, 16.0, "embedded truss anchor"),
        ("META20", 1.25, 18, 20.0, "embedded truss anchor"),
        ("META24", 1.25, 18, 24.0, "embedded truss anchor"),
        ("HETA12", 1.25, 16, 12.0, "embedded truss anchor"),
        ("HETA16", 1.25, 16, 16.0, "embedded truss anchor"),
        ("HETA20", 1.25, 16, 20.0, "embedded truss anchor"),
        ("HETA24", 1.25, 16, 24.0, "embedded truss anchor"),
        ("HETA40", 1.25, 16, 40.0, "embedded truss anchor"),
        ("HHETA16", 1.25, 14, 16.0, "embedded truss anchor"),
        ("HHETA20", 1.25, 14, 20.0, "embedded truss anchor"),
        ("HETAL20", 1.25, 16, 20.0, "embedded truss anchor"),
        ("DETAL20", 2.5, 16, 20.0, "embedded truss anchor"),
    ];
    t.iter().find(|r| r.0 == m).map(|r| (r.1, r.2, r.3, r.4))
}

pub fn lumber_actual(size: &str, product: &str) -> Result<(f64, f64), String> {
    let (a, b) = size.split_once(['x', 'X']).ok_or_else(|| format!("size {:?} must look like \"2x6\" (nominal) or \"1.75x9.25\" (actual)", size))?;
    if product == "sawn" {
        let na: i64 = a.trim().parse().map_err(|_| nominal_hint(size))?;
        let nb: i64 = b.trim().parse().map_err(|_| nominal_hint(size))?;
        let t = match na {
            2 => 1.5,
            4 => 3.5,
            6 => 5.5,
            _ => return Err(nominal_hint(size)),
        };
        let d = match (na, nb) {
            (2, 4) | (4, 4) => 3.5,
            (2, 6) | (4, 6) | (6, 6) => 5.5,
            (2, 8) | (4, 8) => 7.25,
            (6, 8) => 7.5,
            (2, 10) | (4, 10) => 9.25,
            (6, 10) => 9.5,
            (2, 12) | (4, 12) => 11.25,
            (6, 12) => 11.5,
            _ => return Err(nominal_hint(size)),
        };
        Ok((t, d))
    } else {
        let t = crate::num::parse_length(a.trim())?;
        let d = crate::num::parse_length(b.trim())?;
        if t <= 0.0 || d <= 0.0 {
            return Err(format!("size {:?}: dimensions must be positive", size));
        }
        Ok((t, d))
    }
}

fn nominal_hint(size: &str) -> String {
    format!(
        "size {:?} is not a sawn nominal size. Use 2x4 2x6 2x8 2x10 2x12, 4x4 4x6 4x8 4x10 4x12, 6x6 6x8 6x10 6x12; for engineered products set product (lvl|psl|lsl|glulam) and give the actual size, e.g. \"1.75x9.25\" (thickness x depth)",
        size
    )
}

pub fn cover_default(ctype: &str, shape: &str) -> Option<Value> {
    let s = match ctype {
        "cmu_wall" => r#"{"bottom":0.5,"sides":1.5,"top":1.5}"#,
        "concrete" if shape != "none" => r#"{"bottom":3,"sides":3,"top":1.5}"#,
        _ => return None,
    };
    serde_json::from_str(s).ok()
}

pub fn build(i: &BuildIn) -> R {
    let mut b = match i.ctype {
        "lumber" => lumber(i)?,
        "panel" => panel(i)?,
        "cmu_wall" => cmu_wall(i)?,
        "concrete" => concrete(i)?,
        "rebar" => rebar(i)?,
        "anchor_bolt" => anchor_bolt(i)?,
        "connector" => connector(i)?,
        "truss" => truss(i)?,
        "membrane" => membrane(i)?,
        "fill" => fill(i)?,
        "insulation" => insulation(i)?,
        "solid" => solid(i)?,
        other => return Err(i.err("type", format!("components/{}/type: unknown component type {:?}", i.id, other))),
    };
    for p in &mut b.prisms {
        p.region.outer = make_ccw(&p.region.outer);
        p.region.holes = p.region.holes.iter().map(make_cw).collect();
    }
    Ok(b)
}

// ---------------------------------------------------------------------------------------------

fn lumber(i: &BuildIn) -> R {
    let size = i.s("size", "");
    let product = i.s("product", "sawn");
    let (t, d) = lumber_actual(&size, &product).map_err(|e| i.err("size", format!("components/{}/size: {}", i.id, e)))?;
    let plies = i.f("plies", 1.0).max(1.0) as usize;
    if plies > 20 {
        return Err(i.err("plies", format!("components/{}/plies: {} is too many plies (limit 20)", i.id, plies)));
    }
    let run = i.s("run", "z");
    let treated = i.b("treated", false);
    let blocking = i.b("blocking", false);
    let default_mat = if product != "sawn" {
        "wood_engineered"
    } else if treated {
        "wood_treated"
    } else {
        "wood"
    };
    let mat = i.material(default_mat);
    let mut b = Built::new(&mat);
    let mut desc;
    let mut prism;
    match run.as_str() {
        "z" => {
            let orient = i.s("orient", "upright");
            let (w, h, up) = if orient == "flat" { (d, t * plies as f64, false) } else { (t * plies as f64, d, true) };
            prism = LPrism::new(None, &mat, rect(0.0, 0.0, w, h));
            for k in 0..plies {
                let kf = k as f64;
                let q = if up { quad(kf * t, 0.0, (kf + 1.0) * t, h) } else { quad(0.0, kf * t, w, (kf + 1.0) * t) };
                prism.marks.push(q);
                if k > 0 {
                    let (a, c) = if up { (pt(kf * t, 0.0), pt(kf * t, h)) } else { (pt(0.0, kf * t), pt(w, kf * t)) };
                    prism.extra.push(ExtraStroke { pts: vec![v(a.x, a.y), v(c.x, c.y)], closed: false, pen: "beyond".into() });
                }
            }
            prism.along_z = true;
            prism.diag_mark = blocking;
            b.z = ZSpec::Run;
            desc = format!("lumber {}", size);
            if plies > 1 {
                desc = format!("lumber ({}) {}", plies, size);
            }
            desc.push_str(&format!("{} {} run z", if treated { " PT" } else { "" }, orient));
        }
        "x" | "y" => {
            if !i.has("length") {
                return Err(i.err("length", format!("components/{}/length: required when run is \"{}\" (inches of member length)", i.id, run)));
            }
            let len = i.f("length", 0.0);
            if len <= 0.0 {
                return Err(i.err("length", format!("components/{}/length: must be > 0", i.id)));
            }
            let face = i.s("face", "wide");
            if plies > 1 && face == "narrow" {
                return Err(i.err("plies", format!("components/{}/plies: built-up members (plies > 1) need face \"wide\" when run is x or y", i.id)));
            }
            let (perp, zt) = if face == "wide" { (d, t * plies as f64) } else { (t, d) };
            let reg = if run == "x" { rect(0.0, 0.0, len, perp) } else { rect(0.0, 0.0, perp, len) };
            prism = LPrism::new(None, &mat, reg);
            b.z = ZSpec::Thick(zt);
            desc = format!("lumber {}{} {}", if plies > 1 { format!("({}) ", plies) } else { String::new() }, size, run);
            desc = format!("{}{} {} run {} L={}", desc, if treated { " PT" } else { "" }, face, run, fmt_ftin(len));
        }
        other => return Err(i.err("run", format!("components/{}/run: {:?} is not allowed. Allowed: z, x, y", i.id, other))),
    }
    b.desc = desc;
    b.prisms.push(prism);
    Ok(b)
}

fn panel(i: &BuildIn) -> R {
    let mat = i.material("osb");
    let t = i.f("thickness", 0.0);
    let len = i.f("length", 0.0);
    if t <= 0.0 {
        return Err(i.err("thickness", format!("components/{}/thickness: required, > 0 (e.g. 0.4375 for 7/16\")", i.id)));
    }
    if len <= 0.0 {
        return Err(i.err("length", format!("components/{}/length: required, > 0 (in-plane extent in inches)", i.id)));
    }
    let run = i.s("run", "x");
    let (w, h) = if run == "x" { (len, t) } else { (t, len) };
    let mut p = LPrism::new(None, &mat, rect(0.0, 0.0, w, h));
    p.along_z = true;
    p.marks.push(quad(0.0, 0.0, w, h));
    let mut b = Built::new(&mat);
    b.desc = format!("panel {} {} x {} run {}", mat, fmt_ftin(t), fmt_ftin(len), run);
    b.prisms.push(p);
    Ok(b)
}

fn cmu_wall(i: &BuildIn) -> R {
    let wn = i.f("width", 8.0);
    let w = match wn {
        x if (x - 6.0).abs() < 1e-6 || (x - 5.625).abs() < 1e-6 => 5.625,
        x if (x - 8.0).abs() < 1e-6 || (x - 7.625).abs() < 1e-6 => 7.625,
        x if (x - 10.0).abs() < 1e-6 || (x - 9.625).abs() < 1e-6 => 9.625,
        x if (x - 12.0).abs() < 1e-6 || (x - 11.625).abs() < 1e-6 => 11.625,
        _ => return Err(i.err("width", format!("components/{}/width: {} is not a CMU size. Use nominal 6, 8, 10 or 12 (actual 5.625, 7.625, 9.625, 11.625)", i.id, fmt_num(wn)))),
    };
    let n = i.f("courses", 0.0) as i64;
    if n < 1 {
        return Err(i.err("courses", format!("components/{}/courses: required, integer >= 1 (number of 8\" courses)", i.id)));
    }
    if n > 200 {
        return Err(i.err("courses", format!("components/{}/courses: {} is too tall (limit 200 courses)", i.id, n)));
    }
    let bb = i.f("bond_beam_courses", 0.0) as i64;
    if bb < 0 || bb > n {
        return Err(i.err("bond_beam_courses", format!("components/{}/bond_beam_courses: {} must be between 0 and courses ({})", i.id, bb, n)));
    }
    let grout = i.s("grout", "reinforced");
    let fs = i.f("face_shell", 1.25);
    if fs * 2.0 >= w {
        return Err(i.err("face_shell", format!("components/{}/face_shell: {} is too thick for a {} wide unit", i.id, fmt_num(fs), fmt_num(w))));
    }
    let top_joint = i.b("top_joint", false);
    let h = n as f64 * 8.0 - 0.375 + if top_joint { 0.375 } else { 0.0 };
    let mut b = Built::new("cmu");
    let mut first_grouted: Option<f64> = None;
    for k in 1..=n {
        let y0 = (k - 1) as f64 * 8.0;
        let y1 = y0 + 7.625;
        let part = format!("course_{}", k);
        let is_bb = k > n - bb;
        let grouted = grout == "solid" || grout == "reinforced" || is_bb;
        b.prisms.push(LPrism::new(Some(&part), "cmu", rect(0.0, y0, fs, y1)));
        b.prisms.push(LPrism::new(Some(&part), "cmu", rect(w - fs, y0, w, y1)));
        if grouted {
            b.prisms.push(LPrism::new(Some(&part), "grout", rect(fs, y0, w - fs, y1)));
            if first_grouted.is_none() {
                first_grouted = Some(y0);
            }
        }
        if k < n || top_joint {
            b.prisms.push(LPrism::new(Some(&format!("joint_{}", k)), "mortar", rect(0.0, y1, w, y1 + 0.375)));
        }
        b.zones.push((part, rect(0.0, y0, w, y1)));
    }
    if bb > 0 {
        let y0 = (n - bb) as f64 * 8.0;
        b.zones.push(("bond_beam".into(), rect(0.0, y0, w, n as f64 * 8.0 - 0.375)));
        b.anchors.push(("bond_beam_center".into(), pt(w * 0.5, (y0 + n as f64 * 8.0 - 0.375) * 0.5)));
    }
    if let Some(y0) = first_grouted {
        b.zones.push(("grout".into(), rect(fs, y0, w - fs, n as f64 * 8.0 - 0.375)));
    }
    b.anchors.push(("top_center".into(), pt(w * 0.5, h)));
    b.anchors.push(("cell_center_top".into(), pt(w * 0.5, h)));
    b.box_ = Some(Rect::new(0.0, 0.0, w, h));
    b.desc = format!(
        "cmu_wall {}\" x {} courses{}",
        fmt_num(wn.round().max(if wn > 6.0 { wn } else { 6.0 })),
        n,
        if bb > 0 { format!(" ({} bond beam)", bb) } else { String::new() }
    );
    // normalise desc width: nominal
    let nominal = if (w - 5.625).abs() < 1e-6 { 6 } else if (w - 7.625).abs() < 1e-6 { 8 } else if (w - 9.625).abs() < 1e-6 { 10 } else { 12 };
    b.desc = format!("cmu_wall {}\" x {} courses{}", nominal, n, if bb > 0 { format!(" ({} bond beam)", bb) } else { String::new() });
    Ok(b)
}

fn concrete(i: &BuildIn) -> R {
    let shape = i.s("shape", "");
    let mat = i.material("concrete");
    let mut b = Built::new(&mat);
    match shape.as_str() {
        "rect" | "footing" => {
            let w = i.f("width", 0.0);
            let h = i.f("height", 0.0);
            if w <= 0.0 || h <= 0.0 {
                return Err(i.err(if w <= 0.0 { "width" } else { "height" }, format!("components/{}: shape {} needs width and height > 0 (inches)", i.id, shape)));
            }
            let part = if shape == "footing" { "footing" } else { "body" };
            b.prisms.push(LPrism::new(Some(part), &mat, rect(0.0, 0.0, w, h)));
            b.zones.push((part.into(), rect(0.0, 0.0, w, h)));
            b.desc = format!("concrete {} {} x {}", shape, fmt_ftin(w), fmt_ftin(h));
        }
        "polygon" => {
            let pts = i.need_pts("shape polygon")?;
            if pts.len() < 3 {
                return Err(i.err("points", format!("components/{}/points: polygon needs at least 3 points", i.id)));
            }
            b.absolute = true;
            b.prisms.push(LPrism::new(Some("body"), &mat, Region::new(pts.clone())));
            b.zones.push(("body".into(), Region::new(pts)));
            b.desc = "concrete polygon".into();
        }
        "slab_edge" => {
            let ext_right = i.s("exterior", "left") == "right";
            let st = i.f("slab_thickness", 4.0);
            let sl = i.f("slab_length", 48.0);
            let fw = i.f("footing_width", 12.0);
            let fd = i.f("footing_depth", 18.0);
            let hang = i.f("haunch", 45.0);
            let rs = i.f("recess_slope", 0.0);
            if st <= 0.0 || fd <= st {
                return Err(i.err("footing_depth", format!("components/{}/footing_depth: {} must exceed slab_thickness {}", i.id, fmt_num(fd), fmt_num(st))));
            }
            if hang <= 0.0 || hang > 90.0 {
                return Err(i.err("haunch", format!("components/{}/haunch: {} must be in (0, 90] degrees from horizontal", i.id, fmt_num(hang))));
            }
            let hx = if (hang - 90.0).abs() < 1e-9 { fw } else { fw + (fd - st) / hang.to_radians().tan() };
            if hx >= sl {
                return Err(i.err("slab_length", format!("components/{}/slab_length: {} is too short; the haunch meets the slab underside at x={}", i.id, fmt_num(sl), fmt_num(hx))));
            }
            let mut pts: Vec<Pt> = vec![pt(0.0, -fd), pt(fw, -fd), pt(hx, -st), pt(sl, -st), pt(sl, 0.0)];
            let mut anchors: Vec<(String, Pt)> = vec![
                ("top_exterior".into(), pt(0.0, 0.0)),
                ("slab_top".into(), pt(sl, 0.0)),
                ("footing_bottom_exterior".into(), pt(0.0, -fd)),
                ("footing_bottom_interior".into(), pt(fw, -fd)),
                ("slab_bottom_interior".into(), pt(sl, -st)),
                ("haunch_top".into(), pt(hx, -st)),
            ];
            let mut rdesc = String::new();
            if i.has("recess") {
                let r = i.m.get("recess").and_then(|r| r.as_object());
                let g = |k: &str, d: f64| r.and_then(|r| r.get(k)).and_then(|v| v.as_f64()).unwrap_or(d);
                let (rw, rd, re) = (g("width", 0.0), g("depth", 0.0), g("from_edge", 0.0));
                if rw <= 0.0 || rd <= 0.0 {
                    return Err(i.err("recess", format!("components/{}/recess: needs width > 0 and depth > 0 (inches); from_edge defaults to 0", i.id)));
                }
                if rd + rs >= st {
                    return Err(i.err("recess", format!("components/{}/recess: depth {} + recess_slope {} must be less than slab_thickness {}", i.id, fmt_num(rd), fmt_num(rs), fmt_num(st))));
                }
                if re + rw >= sl {
                    return Err(i.err("recess", format!("components/{}/recess: from_edge + width ({}) must be less than slab_length {}", i.id, fmt_num(re + rw), fmt_num(sl))));
                }
                pts.push(pt(re + rw, 0.0));
                pts.push(pt(re + rw, -rd));
                pts.push(pt(re, -(rd + rs)));
                if re > 1e-9 {
                    pts.push(pt(re, 0.0));
                }
                anchors.push(("recess_bottom_exterior".into(), pt(re, -(rd + rs))));
                anchors.push(("recess_bottom_interior".into(), pt(re + rw, -rd)));
                anchors.push(("recess_top_interior".into(), pt(re + rw, 0.0)));
                rdesc = format!(", recess {} x {}", fmt_ftin(rw), fmt_ftin(rd));
            }
            let loop_: Loop = pts.iter().map(|p| v(p.x, p.y)).collect();
            b.prisms.push(LPrism::new(Some("slab_edge"), &mat, Region::new(loop_)));
            b.zones.push(("footing".into(), rect(0.0, -fd, fw, -st)));
            b.zones.push(("slab".into(), rect(0.0, -st, sl, 0.0)));
            b.anchors = anchors;
            b.box_ = Some(Rect::new(0.0, -fd, sl, 0.0));
            b.desc = format!("concrete slab_edge {} slab, ftg {} x {}{}", fmt_ftin(st), fmt_ftin(fw), fmt_ftin(fd), rdesc);
            if ext_right {
                b.transform(&Xf::mirror_x(0.0));
            }
        }
        other => return Err(i.err("shape", format!("components/{}/shape: {:?} is not a concrete shape. Allowed: rect, footing, polygon, slab_edge", i.id, other))),
    }
    Ok(b)
}

fn circle_region(c: Pt, r: f64) -> Region {
    Region::new(vec![vb(c.x - r, c.y, 1.0), vb(c.x + r, c.y, 1.0)])
}

fn rebar(i: &BuildIn) -> R {
    let size = i.s("size", "#4");
    let Some(dia) = rebar_dia(&size) else {
        return Err(i.err("size", format!("components/{}/size: {:?} is not a bar size. Allowed: #3 #4 #5 #6 #7 #8", i.id, size)));
    };
    let r = dia * 0.5;
    let mode = i.s("mode", "along_z");
    let mut b = Built::new("rebar");
    b.embedded = true;
    if mode == "along_z" {
        b.z = ZSpec::Run;
        if i.has("place") {
            let pl = i.m.get("place").and_then(|p| p.as_object()).cloned().unwrap_or_default();
            let host = i
                .host
                .as_ref()
                .ok_or_else(|| i.err("place", format!("components/{}/place: host zone could not be resolved", i.id)))?;
            let bb = region_bbox(&host.region);
            let face = pl.get("face").and_then(|f| f.as_str()).unwrap_or("bottom");
            let cover = pl.get("cover").and_then(|f| f.as_f64()).unwrap_or(1.5);
            let count = pl.get("count").and_then(|f| f.as_f64()).unwrap_or(1.0).max(1.0) as usize;
            if count > 200 {
                return Err(i.err("place", format!("components/{}/place/count: {} bars is too many (limit 200)", i.id, count)));
            }
            let side = pl.get("side_cover").and_then(|f| f.as_f64()).unwrap_or(cover);
            let (horizontal, fixed, lo, hi) = match face {
                "bottom" => (true, bb.y0 + cover + r, bb.x0 + side + r, bb.x1 - side - r),
                "top" => (true, bb.y1 - cover - r, bb.x0 + side + r, bb.x1 - side - r),
                "left" => (false, bb.x0 + cover + r, bb.y0 + side + r, bb.y1 - side - r),
                _ => (false, bb.x1 - cover - r, bb.y0 + side + r, bb.y1 - side - r),
            };
            if hi < lo - 1e-9 {
                return Err(i.err(
                    "place",
                    format!(
                        "components/{}/place: zone is too small for {} bars with side_cover {}: usable span {} (zone box x {}..{}, y {}..{}). Reduce side_cover or use a larger zone",
                        i.id,
                        size,
                        fmt_num(side),
                        fmt_num(hi - lo),
                        fmt_ftin(bb.x0),
                        fmt_ftin(bb.x1),
                        fmt_ftin(bb.y0),
                        fmt_ftin(bb.y1)
                    ),
                ));
            }
            for k in 0..count {
                let u = if count == 1 { (lo + hi) * 0.5 } else { lo + (hi - lo) * k as f64 / (count - 1) as f64 };
                let c = if horizontal { pt(u, fixed) } else { pt(fixed, u) };
                let part = if count > 1 { Some(format!("bar_{}", k + 1)) } else { None };
                let mut p = LPrism::new(part.as_deref(), "rebar", circle_region(c, r));
                p.bar = Some((c, r));
                p.fill_solid = true;
                p.pen = Some("rebar".into());
                p.embedded = Some(true);
                b.prisms.push(p);
            }
            b.absolute = true;
            b.desc = format!("rebar ({}) {} along z @ {} face", count, size, face);
        } else {
            let mut p = LPrism::new(None, "rebar", circle_region(pt(0.0, 0.0), r));
            p.bar = Some((pt(0.0, 0.0), r));
            p.fill_solid = true;
            p.pen = Some("rebar".into());
            p.embedded = Some(true);
            b.prisms.push(p);
            b.box_ = Some(Rect::new(-r, -r, r, r));
            b.desc = format!("rebar {} along z", size);
        }
    } else {
        let pts = i.need_pts("rebar mode path")?;
        let ps: Vec<Pt> = pts.iter().map(|q| q.p()).collect();
        let inner = i.f("bend_radius", 3.0 * dia);
        let rc = inner + r;
        let outline = stroke_bar(&ps, dia, rc);
        let mut p = LPrism::new(None, "rebar", Region::new(outline));
        p.pen = Some("rebar".into());
        p.fill_solid = false;
        p.embedded = Some(true);
        p.sweep = Some((bar_centerline(&ps, rc, 0.01), r));
        b.prisms.push(p);
        b.z = ZSpec::Thick(dia);
        b.absolute = true;
        b.desc = format!("rebar {} path L={}", size, fmt_ftin(ps.windows(2).map(|w| w[0].dist(w[1])).sum()));
    }
    if let Some(n) = i.m.get("spacing_note").and_then(|n| n.as_str()) {
        b.desc.push_str(&format!(" ({})", n));
    }
    Ok(b)
}

fn anchor_bolt(i: &BuildIn) -> R {
    let d = i.f("diameter", 0.5);
    let embed = i.f("embed", 0.0);
    let proj = i.f("projection", 0.0);
    if d <= 0.0 {
        return Err(i.err("diameter", format!("components/{}/diameter: must be > 0 (0.5 or 0.625 typical)", i.id)));
    }
    if embed <= 0.0 {
        return Err(i.err("embed", format!("components/{}/embed: required, > 0 (inches below the placement point)", i.id)));
    }
    if proj <= 0.0 {
        return Err(i.err("projection", format!("components/{}/projection: required, > 0 (inches above the placement point)", i.id)));
    }
    let hook = i.s("hook", "J");
    let hook_len = 3.0;
    let rc_hook = 1.0_f64.max(d * 1.5);
    let mut path: Vec<Pt> = vec![pt(0.0, proj), pt(0.0, -embed)];
    let mut head: Option<Region> = None;
    match hook.as_str() {
        "L" => path.push(pt(rc_hook + hook_len, -embed)),
        "J" => {
            path.push(pt(2.0 * rc_hook, -embed));
            path.push(pt(2.0 * rc_hook, -embed + hook_len));
        }
        "headed" => head = Some(rect(-d, -embed - 0.3 * d, d, -embed + 0.3 * d)),
        _ => {}
    }
    let rc = 1.0_f64.max(d * 1.5);
    let outline = stroke_bar(&path, d, rc);
    let mut b = Built::new("steel");
    b.embedded = true;
    b.z = ZSpec::Thick(d);
    let mut p = LPrism::new(Some("shank"), "steel", Region::new(outline));
    p.pen = Some("steel".into());
    p.fill_solid = true;
    p.embedded = Some(true);
    b.prisms.push(p);
    if let Some(h) = head {
        let mut p = LPrism::new(Some("head"), "steel", h);
        p.pen = Some("steel".into());
        p.fill_solid = true;
        p.embedded = Some(true);
        b.prisms.push(p);
    }
    if i.b("nut_washer", true) {
        let nut_h = 0.875 * d;
        let nut_w = 1.5 * d;
        let wash_t = 0.125;
        let wash_w = 3.0 * d;
        let mut w = LPrism::new(Some("washer"), "steel", rect(-wash_w * 0.5, proj - nut_h - wash_t, wash_w * 0.5, proj - nut_h));
        w.pen = Some("steel".into());
        w.fill_solid = true;
        w.embedded = Some(true);
        b.prisms.push(w);
        let mut n = LPrism::new(Some("nut"), "steel", rect(-nut_w * 0.5, proj - nut_h, nut_w * 0.5, proj));
        n.pen = Some("steel".into());
        n.fill_solid = true;
        n.embedded = Some(true);
        b.prisms.push(n);
    }
    b.anchors.push(("top_of_concrete".into(), pt(0.0, 0.0)));
    b.desc = format!("anchor_bolt {}\" dia, embed {}, proj {}, {} hook", fmt_num(d), fmt_ftin(embed), fmt_ftin(proj), hook);
    Ok(b)
}

fn connector(i: &BuildIn) -> R {
    let pts = i.need_pts("a connector")?;
    let ps: Vec<Pt> = pts.iter().map(|q| q.p()).collect();
    let model = i.s("model", "");
    let hw = if model.is_empty() { None } else { hardware(&model) };
    if !model.is_empty() && hw.is_none() {
        // unknown model: fine, geometry from params; note carries the model
    }
    let gauge = if i.has("gauge") { i.f("gauge", 18.0) as i64 } else { hw.map(|h| h.1).unwrap_or(18) };
    let Some(t) = gauge_t(gauge) else {
        return Err(i.err("gauge", format!("components/{}/gauge: {} is not a known gauge. Allowed: 10 12 14 16 18 20", i.id, gauge)));
    };
    let width = if i.has("width") { i.f("width", 1.25) } else { hw.map(|h| h.0).unwrap_or(1.25) };
    let lay = i.s("lay", "edge");
    let side_left = i.s("side", "left") == "left";
    let mut b = Built::new("steel");
    b.absolute = true;
    let region = if lay == "face" {
        b.z = ZSpec::Thick(t);
        Region::new(centered_strip(&ps, width))
    } else {
        b.z = ZSpec::Thick(width);
        Region::new(thick_poly(&ps, if side_left { t } else { -t }))
    };
    let mut p = LPrism::new(None, "steel", region);
    p.pen = Some("steel".into());
    p.fill_solid = true;
    b.prisms.push(p);
    b.desc = format!(
        "connector {}{} {} ga x {} lay {}",
        if model.is_empty() { String::new() } else { format!("{} ", model) },
        if let Some(h) = hw { h.3 } else { "" },
        gauge,
        fmt_ftin(width),
        lay
    )
    .replace("  ", " ");
    Ok(b)
}

fn truss(i: &BuildIn) -> R {
    let ext_right = i.s("exterior", "left") == "right";
    let pitch_v = i.m.get("pitch").cloned().unwrap_or(Value::String("4:12".into()));
    let th = parse_slope(&pitch_v).map_err(|e| i.err("pitch", format!("components/{}/pitch: {}", i.id, e)))?.to_radians();
    let m = th.tan();
    let tc = i.s("top_chord", "2x4");
    let bc = i.s("bottom_chord", "2x4");
    let (tt, td) = lumber_actual(&tc, "sawn").map_err(|e| i.err("top_chord", format!("components/{}/top_chord: {}", i.id, e)))?;
    let (_bt, bd) = lumber_actual(&bc, "sawn").map_err(|e| i.err("bottom_chord", format!("components/{}/bottom_chord: {}", i.id, e)))?;
    let heel = i.s("heel", "standard");
    let bearing = i.f("bearing_width", 3.5);
    let ov = i.f("overhang", 12.0);
    let span = i.f("span_shown", 48.0);
    let tail = i.s("tail", "plumb");
    if span <= bearing {
        return Err(i.err("span_shown", format!("components/{}/span_shown: {} must exceed bearing_width {}", i.id, fmt_num(span), fmt_num(bearing))));
    }
    let vdepth = td / th.cos();
    let gap = if heel == "raised" {
        let hh = i.f("heel_height", 5.5);
        if hh < vdepth {
            return Err(i.err(
                "heel_height",
                format!("components/{}/heel_height: {} is less than the top chord's vertical depth {} at this pitch; use heel standard or raise heel_height", i.id, fmt_num(hh), fmt_num(vdepth)),
            ));
        }
        hh - vdepth
    } else {
        0.0
    };
    let y0 = bd + gap; // lower edge of top chord at x = 0
    let ly = |x: f64| y0 + m * x;
    let uy = |x: f64| y0 + m * x + vdepth;
    let (tail_bottom, tail_top, tcloop);
    if tail == "square" {
        let l = pt(-ov, ly(-ov));
        let n = pt(-th.sin(), th.cos()) * td;
        let u = l + n;
        tail_bottom = l;
        tail_top = u;
        tcloop = vec![v(l.x, l.y), v(span, ly(span)), v(span, uy(span)), v(u.x, u.y)];
    } else {
        tail_bottom = pt(-ov, ly(-ov));
        tail_top = pt(-ov, uy(-ov));
        tcloop = vec![v(-ov, ly(-ov)), v(span, ly(span)), v(span, uy(span)), v(-ov, uy(-ov))];
    }
    let mut b = Built::new("wood");
    b.z = ZSpec::Thick(tt);
    let mut top = LPrism::new(Some("top_chord"), "wood", Region::new(tcloop));
    top.outline = Outline::Full;
    b.prisms.push(top);
    b.prisms.push(LPrism::new(Some("bottom_chord"), "wood", rect(0.0, 0.0, span, bd)));
    if gap > 1e-9 {
        let w = 1.5;
        let web = vec![v(0.0, bd), v(w, bd), v(w, ly(w)), v(0.0, ly(0.0))];
        b.prisms.push(LPrism::new(Some("heel_web"), "wood", Region::new(web)));
    }
    // tail zone: the last stretch of the top chord
    let tl = 3.0;
    let tz = vec![v(tail_bottom.x, tail_bottom.y), v(tail_bottom.x + tl, ly(tail_bottom.x + tl)), v(tail_bottom.x + tl, uy(tail_bottom.x + tl)), v(tail_top.x, tail_top.y)];
    b.zones.push(("tail".into(), Region::new(tz)));
    if i.b("plate", true) {
        let (px0, px1) = (0.5, (0.5 + 5.0_f64).min(span - 0.25));
        let (py0, py1) = (0.5, (y0 + m * px1 - 0.3).max(1.0));
        let pl = vec![v(px0, py0), v(px1, py0), v(px1, py1), v(px0, py1)];
        b.zones.push(("plate".into(), Region::new(pl.clone())));
        let mut ex = vec![];
        ex.push(ExtraStroke { pts: pl, closed: true, pen: "hidden".into() });
        // attach the dashed outline to the bottom chord prism so it follows its visibility
        b.prisms[1].extra = ex;
    }
    b.anchors = vec![
        ("bearing_outer".into(), pt(0.0, 0.0)),
        ("bearing_inner".into(), pt(bearing, 0.0)),
        ("tail_bottom".into(), tail_bottom),
        ("tail_top".into(), tail_top),
        ("top_chord_at_bearing".into(), pt(0.0, uy(0.0))),
        ("top_chord_end".into(), pt(span, uy(span))),
        ("bottom_chord_top_inner".into(), pt(span, bd)),
    ];
    b.desc = format!("truss {} {} heel, {}+{} chords, ovh {}", crate::num::fmt_num(m * 12.0).to_string() + ":12", heel, tc, bc, fmt_ftin(ov));
    if ext_right {
        b.transform(&Xf::mirror_x(0.0));
    }
    Ok(b)
}

fn membrane(i: &BuildIn) -> R {
    let mat = i.material("underlayment");
    let pts = i.need_pts("a membrane")?;
    let ps: Vec<Pt> = pts.iter().map(|q| q.p()).collect();
    let t = if i.has("thickness") {
        i.f("thickness", 0.04)
    } else {
        match mat.as_str() {
            "shingles" => 0.25,
            "wrb" => 0.03,
            _ => 0.04,
        }
    };
    let left = i.s("side", "left") == "left";
    let mut b = Built::new(&mat);
    b.absolute = true;
    let mut p = LPrism::new(None, &mat, Region::new(thick_poly(&ps, if left { t } else { -t })));
    p.pen = i.style.material(&mat).pen.clone().or(Some("membrane".into()));
    p.center = Some((ps.clone(), if left { t } else { -t }));
    b.prisms.push(p);
    b.desc = format!("membrane {} {} thick L={}", mat, fmt_ftin(t), fmt_ftin(ps.windows(2).map(|w| w[0].dist(w[1])).sum()));
    Ok(b)
}

fn fill(i: &BuildIn) -> R {
    let mat = i.material("earth");
    let pts = i.need_pts("a fill")?;
    if pts.len() < 3 {
        return Err(i.err("points", format!("components/{}/points: a fill polygon needs at least 3 points", i.id)));
    }
    let mut b = Built::new(&mat);
    b.absolute = true;
    let mut p = LPrism::new(None, &mat, Region::new(pts));
    p.outline = match i.s("outline", "top").as_str() {
        "full" => Outline::Full,
        "none" => Outline::None,
        _ => Outline::Top,
    };
    b.prisms.push(p);
    b.desc = format!("fill {}", mat);
    Ok(b)
}

fn insulation(i: &BuildIn) -> R {
    let form = i.s("form", "rigid");
    let mat = if form == "batt" { "insulation_batt" } else { "insulation_rigid" };
    let mat = i.material(mat);
    let mut b = Built::new(&mat);
    let (region, bbox) = if i.has("points") {
        let pts = i.need_pts("insulation points")?;
        b.absolute = true;
        let r = Region::new(pts);
        let bb = region_bbox(&r);
        (r, bb)
    } else {
        let (w, h) = (i.f("width", 0.0), i.f("height", 0.0));
        if w <= 0.0 || h <= 0.0 {
            return Err(i.err("width", format!("components/{}: insulation needs width and height > 0, or points", i.id)));
        }
        (rect(0.0, 0.0, w, h), Rect::new(0.0, 0.0, w, h))
    };
    let mut p = LPrism::new(None, &mat, region);
    if form == "batt" {
        // sinusoidal loop line along the long axis
        let (w, h) = (bbox.w(), bbox.h());
        let horiz = w >= h;
        let (len, amp) = if horiz { (w, h * 0.5 - 0.0) } else { (h, w * 0.5) };
        let n = ((len / (amp * 1.6)).round() as usize).max(2);
        let mut pts = vec![];
        let steps = n * 24;
        for s in 0..=steps {
            let t = s as f64 / steps as f64 * n as f64 * std::f64::consts::TAU;
            // prolate cycloid: loops
            let along = (t - 0.9 * t.sin()) / (n as f64 * std::f64::consts::TAU) * len;
            let across = amp * (1.0 - 0.9 * t.cos()) * 0.5 + amp * 0.05;
            let across = across.min(amp * 2.0 * 0.98);
            let q = if horiz { pt(bbox.x0 + along, bbox.y0 + across * (h / (amp * 2.0)).min(1.0) * 1.0) } else { pt(bbox.x0 + across, bbox.y0 + along) };
            pts.push(v(q.x, q.y));
        }
        p.extra.push(ExtraStroke { pts, closed: false, pen: "beyond".into() });
    }
    b.prisms.push(p);
    b.box_ = Some(bbox);
    b.desc = format!("insulation {}", form);
    Ok(b)
}

fn solid(i: &BuildIn) -> R {
    let mat = i.material("generic");
    let prof = i.m.get("profile").and_then(|p| p.as_object()).cloned().unwrap_or_default();
    let mut b = Built::new(&mat);
    let region = if let Some(r) = prof.get("rect").and_then(|r| r.as_array()) {
        let (w, h) = (r.first().and_then(|x| length_of(x).ok()).unwrap_or(0.0), r.get(1).and_then(|x| length_of(x).ok()).unwrap_or(0.0));
        if w <= 0.0 || h <= 0.0 {
            return Err(i.err("profile", format!("components/{}/profile/rect: width and height must be > 0", i.id)));
        }
        rect(0.0, 0.0, w, h)
    } else if let Some(d) = prof.get("circle").and_then(|d| d.as_f64()) {
        if d <= 0.0 {
            return Err(i.err("profile", format!("components/{}/profile/circle: diameter must be > 0", i.id)));
        }
        b.box_ = Some(Rect::new(0.0, 0.0, d, d));
        circle_region(pt(d * 0.5, d * 0.5), d * 0.5)
    } else {
        let pts = i.need_pts("solid profile points")?;
        if pts.len() < 3 {
            return Err(i.err("profile", format!("components/{}/profile/points: needs at least 3 points", i.id)));
        }
        b.absolute = true;
        Region::new(pts)
    };
    let st = i.style.material(&mat);
    let mut p = LPrism::new(None, &mat, region);
    p.fill_solid = st.fill;
    if st.fill {
        p.pen = Some("steel".into());
    }
    b.prisms.push(p);
    b.desc = format!("solid {} (escape hatch)", mat);
    Ok(b)
}
