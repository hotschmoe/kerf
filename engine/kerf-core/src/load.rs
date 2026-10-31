//! A loaded document: compiled scene + diagnostics, plus summary text and `inspect` queries.

use crate::catalog;
use crate::compile;
use crate::coverage;
use crate::crop::{self, Before};
use crate::drawview;
use crate::geom::{self, Box, V2};
use crate::json::{self, Member, Value};
use crate::layout;
use crate::limits;
use crate::lint::{self, AckMode, Acked};
use crate::model::{self, Diag, Diags, Level};
use crate::scene::{self, Comp, Scene, State};
use crate::section::{Class, Section};
use crate::sort;
use crate::style::Style;
use crate::units;
use crate::validate;
use crate::view;
use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct Loaded<'a> {
    pub doc: &'a Value,
    pub style: &'a Style,
    pub scene: Scene<'a>,
    pub diags: &'a Diags,
    pub nviews: usize,
    /// `meta.requested` coverage (SPEC 21, 25.3), computed once with the compiled scene.
    pub coverage: Vec<coverage::Item>,
    /// The warnings `acknowledge` entries suppressed (in strict mode they are also still in `diags`).
    pub acked: Vec<Acked>,
    pub strict: bool,
}

pub fn load<'a>(doc: &'a Value, st: &'a Style, diags: &'a Diags, with_views: bool) -> Loaded<'a> {
    load_after_edit(doc, st, diags, with_views, None, false)
}

/// Compile, validate and lint `doc`; with `with_views` also build every view so view diagnostics are included.
/// `before` is the loaded document prior to an edit (the crop check compares against it). `strict` keeps
/// acknowledged warnings as warnings (`kerf check --strict`).
pub fn load_after_edit<'a>(
    doc: &'a Value,
    st: &'a Style,
    diags: &'a Diags,
    with_views: bool,
    before: Option<&Loaded<'_>>,
    strict: bool,
) -> Loaded<'a> {
    let scene = compile::compile(doc, st, diags);
    let is_obj = matches!(doc, Value::Object(_));
    let coverage = if is_obj { coverage::compute(doc, st, Some(&scene)) } else { Vec::new() };
    if is_obj {
        validate::run(&scene, doc, diags);
        lint::run(&scene, doc, &coverage, diags);
        layout::check(doc, diags);
        let prior = before.map(|b| Before { scene: &b.scene, doc: b.doc });
        crop::run(&scene, doc, prior.as_ref(), diags);
    }
    let mut nviews = 0;
    let vdoc = if with_views && is_obj { lint::with_dim_dirs(&scene, doc) } else { Cow::Borrowed(doc) };
    if is_obj && let Some(va) = vdoc.get("views").and_then(Value::arr) {
        nviews = va.len();
        let views: &[Value] = if va.len() > limits::MAX_VIEWS { &[] } else { va };
        let mut seen: Vec<&str> = Vec::new();
        for (i, v) in views.iter().enumerate() {
            let vid = v.get("id").map_or("", |x| x.str().unwrap_or(""));
            if seen.contains(&vid) {
                diags.add(
                    Level::Error,
                    "E_DUP_ID",
                    Some(vid),
                    Some(&format!("views/{i}")),
                    format!("duplicate view id '{vid}'"),
                );
                continue;
            }
            seen.push(vid);
            let vd = Diags::new();
            if let Some(spec) = layout::view_spec(&vdoc, v, i, &vd)
                && with_views
            {
                drawview::build_from_scene(&vdoc, st, &scene, &spec, &vd);
            }
            diags.extend(vd.into_vec());
        }
        if with_views && views.len() == va.len() {
            for lid in layout::layout_ids(&vdoc) {
                let ld = Diags::new();
                layout::build_from_scene(&vdoc, st, &scene, lid, &ld);
                let known = diags.to_vec();
                diags.extend(ld.into_vec().into_iter().filter(|x| !known.contains(x)));
            }
        }
    }
    let mut acked = Vec::new();
    if is_obj {
        let complete = with_views && nviews <= limits::MAX_VIEWS && diags.err_count() == 0;
        acked = lint::apply_acknowledge(doc, diags, AckMode { strict, complete });
    }
    Loaded { doc, style: st, scene, diags, nviews, coverage, acked, strict }
}

/// The acknowledged warnings on components view `view_id` draws: the document-level checks plus `view_diags` (that
/// view's own build diagnostics), through `acknowledge`.
pub fn view_acked(doc: &Value, st: &Style, view_id: &str, view_diags: &[Diag]) -> Vec<Acked> {
    let diags = Diags::new();
    let scene = compile::compile(doc, st, &diags);
    validate::run(&scene, doc, &diags);
    lint::run(&scene, doc, &[], &diags);
    crop::run(&scene, doc, None, &diags);
    diags.extend(view_diags.iter().cloned());
    let acked = lint::apply_acknowledge(doc, &diags, AckMode { strict: false, complete: false });
    let Some(node) = view::find_view(doc, view_id) else { return Vec::new() };
    let members = view::Members::of(node);
    acked
        .into_iter()
        .filter(|a| scene.find(&a.comp).is_some_and(|c| c.state == State::Ok && c.visible && members.draws(c)))
        .collect()
}

fn pad_to(out: &mut String, s: &str, n: usize) {
    out.push_str(s);
    if s.len() >= n {
        out.push(' ');
        return;
    }
    for _ in s.len()..n {
        out.push(' ');
    }
}

pub fn comp_box(c: &Comp<'_>) -> Box {
    let mut b = Box::default();
    for p in &c.world {
        for l in &p.loops {
            b.add_box(geom::loop_box(l));
        }
    }
    b
}

pub fn diag_line(d: &Diag) -> String {
    let lvl = match d.level {
        Level::Error => "ERROR",
        Level::Warning => "WARN",
        Level::Info => "INFO",
    };
    let mut out = format!("{lvl} {}", d.code);
    if let Some(id) = d.id.as_deref().filter(|s| !s.is_empty()) {
        out.push(' ');
        out.push_str(id);
    }
    out.push_str(": ");
    out.push_str(&d.message);
    if let Some(f) = &d.fix {
        out.push_str(" Fix: ");
        out.push_str(f);
    }
    out
}

fn coverage_block(out: &mut String, l: &Loaded) {
    let items = &l.coverage;
    if items.is_empty() {
        return;
    }
    let covered = items.iter().filter(|it| !it.found.is_empty()).count();
    let tail = if covered == items.len() { "" } else { "  (finish only at full coverage)" };
    out.push_str(&format!("COVERAGE  {covered}/{} requested elements covered{tail}\n", items.len()));
    for it in items {
        out.push(' ');
        pad_to(out, if it.found.is_empty() { "MISSING" } else { "ok" }, 9);
        if it.found.is_empty() {
            match &it.closest {
                Some(c) => {
                    pad_to(out, &it.text, 26);
                    let lacks: Vec<&str> = c.missing.iter().map(String::as_str).collect();
                    out.push_str(&format!("closest: {} (lacks {})", c.source, model::join_quoted(&lacks)));
                }
                None => out.push_str(&it.text),
            }
            out.push('\n');
            continue;
        }
        pad_to(out, &it.text, 26);
        for (i, id) in it.found.iter().enumerate() {
            if i == 6 {
                out.push_str(&format!(", +{} more", it.found.len() - 6));
                break;
            }
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(id);
        }
        out.push('\n');
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub fn summary(l: &Loaded) -> String {
    summary_with(l, &l.diags.list())
}

/// The summary text of `l` listing `diags` instead of `l.diags` (apply merges the op diagnostics in).
pub fn summary_with(l: &Loaded, diags: &[Diag]) -> String {
    let e = diags.iter().filter(|d| d.level == Level::Error).count();
    let w = diags.iter().filter(|d| d.level == Level::Warning).count();
    let id = l.doc.get("id").map_or("", |x| x.str().unwrap_or(""));
    let nc = l.scene.comps.len();
    let a = l.acked.len();
    let mut out = format!(
        "DOC {id}  {nc} component{}  {} view{}  {e} error{}  {w} warning{}  {a} acknowledged{}\n",
        plural(nc),
        l.nviews,
        plural(l.nviews),
        plural(e),
        plural(w),
        if l.strict && a > 0 { " (--strict: listed as warnings)" } else { "" }
    );
    for c in &l.scene.comps {
        out.push(' ');
        pad_to(&mut out, &c.id, 14);
        if c.state != State::Ok {
            pad_to(&mut out, c.ty.name(), 44);
            out.push_str("(not built: see errors)\n");
            continue;
        }
        let mut desc = c.built.info.clone();
        if c.arr_count > 1
            && let Some(av) = c.node.get("array")
        {
            let ax = av.get("axis").map_or("x", |x| x.str().unwrap_or("x"));
            let sp = av.get("spacing").map_or(0.0, |x| units::parse_length(x).unwrap_or(0.0));
            desc.push_str(&format!(" [x{} {ax} @ {}]", c.arr_count, units::fmt_ft_in(sp)));
        }
        if let Some(lb) = &c.label {
            desc.push_str(&format!(" \"{lb}\""));
        }
        pad_to(&mut out, &desc, 44);
        let b = comp_box(c);
        let mut xr = String::from("x ");
        units::append_ft_in(&mut xr, b.x0);
        xr.push_str("..");
        units::append_ft_in(&mut xr, b.x1);
        pad_to(&mut out, &xr, 18);
        out.push_str("y ");
        units::append_ft_in(&mut out, b.y0);
        out.push_str("..");
        units::append_ft_in(&mut out, b.y1);
        out.push('\n');
    }
    if matches!(l.doc, Value::Object(_)) {
        coverage_block(&mut out, l);
    }
    for d in diags {
        if d.level == Level::Info
            && !matches!(
                d.code,
                "I_SOLID_USED" | "I_UNVERIFIED_CITE" | "I_CITE_DOWNGRADED" | "I_ANNOT_MOVED" | "I_KEY_RENUMBERED"
            )
        {
            continue;
        }
        out.push_str(&diag_line(d));
        out.push('\n');
    }
    if !l.strict {
        acked_block(&mut out, &l.acked);
    }
    out
}

/// The suppressed warnings grouped by code and reason: one line each with the count and the components.
fn acked_block(out: &mut String, acked: &[Acked]) {
    if acked.is_empty() {
        return;
    }
    let mut groups: Vec<(&str, &str, usize, Vec<&str>)> = Vec::new();
    for a in acked {
        match groups.iter_mut().find(|g| g.0 == a.warning.code && g.1 == a.reason) {
            Some(g) => {
                g.2 += 1;
                if !g.3.contains(&a.comp.as_str()) {
                    g.3.push(&a.comp);
                }
            }
            None => groups.push((a.warning.code, &a.reason, 1, alloc::vec![a.comp.as_str()])),
        }
    }
    sort::stable(&mut groups, |x, y| x.0 < y.0);
    out.push_str(&format!(
        "ACKNOWLEDGED  {} warning{} suppressed by `acknowledge` (kerf check --strict prints each in full)\n",
        acked.len(),
        plural(acked.len())
    ));
    for (code, reason, n, comps) in &groups {
        out.push(' ');
        pad_to(out, code, 20);
        pad_to(out, &format!("x{n}"), 5);
        for (i, id) in comps.iter().enumerate() {
            if i == 6 {
                out.push_str(&format!(", +{} more", comps.len() - 6));
                break;
            }
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(id);
        }
        out.push_str(": \"");
        out.push_str(reason);
        out.push_str("\"\n");
    }
}

pub fn diags_json(list: &[Diag]) -> Value {
    Value::Array(list.iter().map(model::diag_to_json).collect())
}

fn s(x: &str) -> Value {
    Value::String(x.to_string())
}

fn anchor_json(name: &str, p: V2) -> Value {
    json::obj([
        ("name", s(name)),
        ("x", Value::Number(p.x)),
        ("y", Value::Number(p.y)),
        ("x_ft", Value::String(units::fmt_ft_in(p.x))),
        ("y_ft", Value::String(units::fmt_ft_in(p.y))),
    ])
}

fn box_anchors(c: &Comp<'_>, part: Option<&str>) -> Value {
    let mut list = Vec::new();
    for n in model::BOX_ANCHOR_NAMES {
        if let Some(p) = Scene::anchor_point(c, 0, part, n) {
            list.push(anchor_json(n, p));
        }
    }
    if part.is_none()
        && let Some(xf) = c.xfs.first()
    {
        for n in &c.built.anchors {
            list.push(anchor_json(&n.name, xf.apply(n.p)));
        }
    }
    Value::Array(list)
}

pub struct InspectError {
    pub code: &'static str,
    pub message: String,
}

fn ierr(code: &'static str, message: impl Into<String>) -> InspectError {
    InspectError { code, message: message.into() }
}

pub fn inspect(l: &Loaded, q: &Value) -> Result<Value, InspectError> {
    let kind = q.get("q").and_then(Value::str).unwrap_or("summary");
    match kind {
        "summary" => Ok(json::obj([("summary", Value::String(summary(l)))])),
        "doc" => Ok(json::obj([("doc", l.doc.clone())])),
        "catalog" => {
            let t = q
                .get("type")
                .and_then(Value::str)
                .ok_or_else(|| ierr("E_PARAM", "inspect catalog needs \"type\", e.g. \"truss\""))?;
            match catalog::find(t) {
                Some(e) => Ok(catalog::entry_json(e)),
                None => Err(ierr(
                    "E_REF_UNKNOWN",
                    format!("unknown component type \"{t}\". Types: {}", scene::join_ids(&catalog::type_names())),
                )),
            }
        }
        "component" | "anchors" => inspect_component(l, q, kind == "anchors"),
        "at" => inspect_at(l, q),
        _ => {
            Err(ierr("E_PARAM", format!("unknown query \"{kind}\": use summary, component, anchors, at, catalog, doc")))
        }
    }
}

fn inspect_component(l: &Loaded, q: &Value, anchors_only: bool) -> Result<Value, InspectError> {
    let id =
        q.get("id").and_then(Value::str).ok_or_else(|| ierr("E_PARAM", "inspect component/anchors needs \"id\""))?;
    let Some(c) = l.scene.find(id) else {
        let ids = l.scene.comp_ids();
        let hint = model::nearest(id, &ids).map(|n| format!(" Did you mean '{n}'?")).unwrap_or_default();
        return Err(ierr("E_REF_UNKNOWN", format!("no component '{id}'.{hint} Components: {}", scene::join_ids(&ids))));
    };
    if c.state != State::Ok {
        return Err(ierr("E_PARAM", format!("component '{id}' could not be built; fix its errors (see check)")));
    }
    let anchors = box_anchors(c, None);
    if anchors_only {
        return Ok(json::obj([("id", s(id)), ("anchors", anchors)]));
    }
    let params: Vec<Member> = match c.node {
        Value::Object(ms) => ms.iter().filter(|m| m.key != "id" && m.key != "type").cloned().collect(),
        _ => Vec::new(),
    };
    let parts: Vec<Value> = l
        .scene
        .part_names(c)
        .into_iter()
        .map(|pn| {
            let is_zone = c.built.zones.iter().any(|z| z.name == pn);
            json::obj([
                ("name", s(pn)),
                ("kind", s(if is_zone { "zone" } else { "prism" })),
                ("anchors", box_anchors(c, Some(pn))),
            ])
        })
        .collect();
    let b = comp_box(c);
    let mat = c.world.first().map_or("", |p| p.material.as_str());
    let z = c.zs.first().copied().unwrap_or([0.0, 0.0]);
    Ok(json::obj([
        ("id", s(id)),
        ("type", s(c.ty.name())),
        ("material", s(mat)),
        ("desc", s(&c.built.info)),
        ("params", Value::Object(params)),
        ("instances", Value::Number(c.xfs.len() as f64)),
        ("z", json::num_arr(&z)),
        ("bbox", json::num_arr(&[b.x0, b.y0, b.x1, b.y1])),
        ("anchors", anchors),
        ("parts", Value::Array(parts)),
    ]))
}

fn inspect_at(l: &Loaded, q: &Value) -> Result<Value, InspectError> {
    let vid = q
        .get("view")
        .and_then(Value::str)
        .ok_or_else(|| ierr("E_PARAM", "inspect at needs \"view\" and \"point\": [x, y]"))?;
    let bad_point = || ierr("E_PARAM", "inspect at needs \"point\": [x, y] in inches");
    let pv = q.get("point").and_then(Value::arr).ok_or_else(bad_point)?;
    let [px, py] = pv else { return Err(bad_point()) };
    let (Some(x), Some(y)) = (units::parse_length(px), units::parse_length(py)) else { return Err(bad_point()) };
    let pt = V2::init(x, y);
    let Some(vnode) = view::find_view(l.doc, vid) else {
        return Err(ierr(
            "E_REF_UNKNOWN",
            format!("unknown view '{vid}'. Views: {}", scene::join_ids(&view::view_ids(l.doc))),
        ));
    };
    let vd = Diags::new();
    let Some(spec_v) = crate::layout::view_spec(l.doc, vnode, 0, &vd) else {
        let msg = vd.list().first().map_or_else(|| "view is invalid".to_string(), |d| d.message.clone());
        return Err(ierr("E_PARAM", msg));
    };
    let resolved = drawview::resolve_spec(l.doc, l.style, &l.scene, &spec_v);
    let mut spec = spec_v.clone();
    spec.crop = resolved.crop;
    spec.scale = resolved.scale;
    let fv = drawview::flat_view(&l.scene, &spec);
    let scene = fv.scene.as_ref().unwrap_or(&l.scene);
    let prisms = &fv.prisms;
    let mut sec = Section::new(scene, &fv.spec, prisms);
    sec.build();
    let mut hits = Vec::new();
    let drawn = Rc::clone(&sec.prisms);
    for (i, p) in drawn.iter().enumerate() {
        if sec.cls[i] == Class::Drop || p.kind != model::Kind::Body {
            continue;
        }
        let reg = sec.visible_region(i);
        if reg.is_empty() || geom::locate_even_odd(pt, &reg, 1e-9) == geom::Loc::Outside {
            continue;
        }
        let Some(c) = scene.comps.get(p.comp as usize) else { continue };
        let src = if c.arr_count > 1 && !c.xfs.is_empty() {
            format!("{}#{}", c.id, p.instance as usize / (c.xfs.len() / c.arr_count).max(1))
        } else {
            c.id.clone()
        };
        hits.push(json::obj([
            ("id", s(&c.id)),
            ("src", Value::String(src)),
            ("part", if p.part.is_empty() { Value::Null } else { s(&p.part) }),
            ("kind", s(if sec.cls[i] == Class::Cut { "cut" } else { "beyond" })),
        ]));
    }
    Ok(json::obj([("view", s(vid)), ("point", json::num_arr(&[pt.x, pt.y])), ("components", Value::Array(hits))]))
}
