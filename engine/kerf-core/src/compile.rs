//! Document -> Scene: validate components, order the placement DAG, run builders, place
//! (translate / rotate / mirror / arrays / z extents) and produce world-space prisms.

use crate::builders::{self, Ctx};
use crate::catalog::{self, Entry};
use crate::geom::{self, V2, Xf};
use crate::json::Value;
use crate::limits;
use crate::model::{self, Built, Diags, Level, Params, Prism};
use crate::num;
use crate::scene::{self, Comp, Scene, State};
use crate::style::Style;
use crate::units;
use crate::view::Members;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const DEFAULT_RUN: [f64; 2] = [-24.0, 24.0];

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    match b.first() {
        Some(c) if c.is_ascii_lowercase() => {
            b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
        }
        _ => false,
    }
}

fn ref_comp_id(s: &str) -> Option<&str> {
    let at = s.find('@')?;
    let mut head = &s[..at];
    if head.is_empty() {
        return None;
    }
    if let Some(d) = head.find('.') {
        head = &head[..d];
    }
    if let Some(h) = head.find('#') {
        head = &head[..h];
    }
    Some(head)
}

fn add_ref_dep<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.extend(ref_comp_id(s)),
        Value::Object(_) => {
            if let Some(r) = v.get("ref") {
                add_ref_dep(r, out);
            }
        }
        _ => {}
    }
}

pub fn collect_deps<'a>(node: &'a Value, out: &mut Vec<&'a str>) {
    if let Some(to) = node.get("at").and_then(|at| at.get("to")) {
        add_ref_dep(to, out);
    }
    if let Some(u) = node.get("until") {
        add_ref_dep(u, out);
    }
    if let Some(ss) = node.get("slope").and_then(Value::str)
        && ss.len() > 1
        && ss.starts_with('@')
    {
        out.push(&ss[1..]);
    }
    if let Some(arr) = node.get("points").and_then(Value::arr) {
        for e in arr {
            add_ref_dep(e, out);
        }
    }
    if let Some(arr) = node.get("profile").and_then(|pr| pr.get("points")).and_then(Value::arr) {
        for e in arr {
            add_ref_dep(e, out);
        }
    }
    if let Some(s) = node.get("place").and_then(|pl| pl.get("in")).and_then(Value::str) {
        out.push(s.split('.').next().unwrap_or(s));
    }
}

/// The components a node names in `voids`, a sleeve's `through` and a fill's `except` (SPEC 26.4). They are not
/// placement dependencies (subtraction runs after placement), but removing one leaves the reference dangling.
pub fn collect_subtract_refs<'a>(node: &'a Value, out: &mut Vec<&'a str>) {
    for v in node.get("voids").and_then(Value::arr).unwrap_or(&[]) {
        match v {
            Value::String(s) => out.push(s.split('.').next().unwrap_or(s)),
            _ => {
                if let Some(a) = v.get("at") {
                    add_ref_dep(a, out);
                }
                for p in v.get("points").and_then(Value::arr).unwrap_or(&[]) {
                    add_ref_dep(p, out);
                }
            }
        }
    }
    for key in ["through", "except"] {
        match node.get(key) {
            Some(Value::String(s)) => out.push(s),
            Some(Value::Array(a)) => out.extend(a.iter().filter_map(Value::str)),
            _ => {}
        }
    }
}

pub fn compile<'a>(doc: &'a Value, st: &'a Style, diags: &'a Diags) -> Scene<'a> {
    let mut scene = Scene::new(st, diags, DEFAULT_RUN);
    if !read_settings(&mut scene, doc) {
        return scene;
    }
    let Some(items) = component_items(doc, diags) else { return scene };
    read_components(&mut scene, items);
    let order = placement_order(&mut scene);
    for i in order {
        if scene.comps[i].state == State::Failed {
            continue;
        }
        place_component(&mut scene, i);
    }
    crate::voids::apply(&mut scene);
    scene
}

/// Stage 1: `run` (default z extent) and the first section view's `cut_z` (default z of in-plane members). False when the
/// document is not an object (reported).
fn read_settings(scene: &mut Scene, doc: &Value) -> bool {
    let diags = scene.diags;
    if !matches!(doc, Value::Object(_)) {
        diags.add(
            Level::Error,
            "E_PARAM",
            None,
            Some(""),
            "the document must be a JSON object with \"kerf\", \"id\", \"components\" and \"views\"".into(),
        );
        return false;
    }
    if let Some(rv) = doc.get("run") {
        let z = rv
            .arr()
            .filter(|a| a.len() == 2)
            .and_then(|a| Some((units::parse_length(&a[0])?, units::parse_length(&a[1])?)));
        match z {
            Some((z0, z1)) if z0 < z1 => scene.run = [z0, z1],
            Some((z0, z1)) => diags.add(
                Level::Error,
                "E_PARAM",
                None,
                Some("run"),
                format!("'run' must be [z0, z1] with z0 < z1 (got [{}, {}])", model::num_text(z0), model::num_text(z1)),
            ),
            None => diags.add(
                Level::Error,
                "E_PARAM",
                None,
                Some("run"),
                format!(
                    "'run' must be [z0, z1] in inches, each a length within +-{} inches, e.g. [-24, 24] (got {})",
                    model::fmt_num(limits::MAX_COORD_IN),
                    model::kind_or_text(rv)
                ),
            ),
        }
    }
    let views = doc.get("views").and_then(Value::arr);
    if let Some(va) = views {
        if va.len() > limits::MAX_VIEWS {
            diags.add_fix(
                Level::Error,
                "E_LIMIT",
                None,
                Some("views"),
                limits::message("views in one document", va.len(), limits::MAX_VIEWS, "No views were built."),
                "keep one view per sheet detail (usually 2 to 6), or split the document".into(),
            );
        }
        for v in va {
            let kind = v.get("kind").map_or("section", |k| k.str().unwrap_or("section"));
            if kind == "iso" || kind == "plan" {
                continue;
            }
            if let Some(z) = v.get("cut_z").and_then(units::parse_length) {
                scene.default_z = Some(z);
            }
            break;
        }
    }
    true
}

/// The `components` array, or None (reported) when it is not an array or has too many entries.
fn component_items<'a>(doc: &'a Value, diags: &Diags) -> Option<&'a [Value]> {
    let items = match doc.get("components") {
        None => return Some(&[]),
        Some(v) => v.arr(),
    };
    let Some(items) = items else {
        diags.add(Level::Error, "E_PARAM", None, Some("components"), "'components' must be an array".into());
        return None;
    };
    if items.len() > limits::MAX_COMPONENTS {
        diags.add_fix(
            Level::Error,
            "E_LIMIT",
            None,
            Some("components"),
            limits::message("components in one document", items.len(), limits::MAX_COMPONENTS, "Nothing was compiled."),
            "split the detail into several documents (one sheet or one assembly each), or merge repeated members into one component with an array".into(),
        );
        return None;
    }
    Some(items)
}

/// Stage 3: every component's id, type and unknown keys; fills `scene.comps` (components with a bad id or type are dropped, reported).
fn read_components<'a>(scene: &mut Scene<'a>, items: &'a [Value]) {
    let diags = scene.diags;
    let mut comps: Vec<Comp<'a>> = Vec::new();
    let type_names = catalog::type_names();
    for (i, item) in items.iter().enumerate() {
        let path = format!("components/{i}");
        let Value::Object(members) = item else {
            diags.add(
                Level::Error,
                "E_PARAM",
                None,
                Some(&path),
                format!("component {i} must be an object with \"id\" and \"type\""),
            );
            continue;
        };
        let Some(id) = item.get("id").and_then(Value::str) else {
            diags.add(
                Level::Error,
                "E_PARAM",
                None,
                Some(&path),
                format!("component {i} needs a string \"id\" matching [a-z][a-z0-9_]*"),
            );
            continue;
        };
        if !valid_id(id) {
            diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&path),
                format!(
                    "id \"{id}\" is invalid: ids must match [a-z][a-z0-9_]* (lowercase letters, digits, underscore)"
                ),
                "rename it, e.g. \"sill_plate\"".into(),
            );
            continue;
        }
        if comps.iter().any(|c| c.id == id) {
            diags.add(
                Level::Error,
                "E_DUP_ID",
                Some(id),
                Some(&path),
                format!("duplicate component id '{id}': ids must be unique across components"),
            );
            continue;
        }
        let Some(ty_name) = item.get("type").and_then(Value::str) else {
            diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("components/{id}/type")),
                format!("component '{id}' needs a \"type\": one of {}", model::join_quoted(&type_names)),
            );
            continue;
        };
        let Some(ty) = catalog::find(ty_name) else {
            let hint =
                model::nearest(ty_name, &type_names).map_or(String::new(), |n| format!(" Did you mean \"{n}\"?"));
            diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("components/{id}/type")),
                format!("unknown component type \"{ty_name}\". Types: {}.{hint}", model::join_quoted(&type_names)),
            );
            continue;
        };
        for m in members {
            if !catalog::allowed_key(ty, &m.key) {
                let allowed = catalog::allowed_keys_text(ty);
                let names = catalog::allowed_key_names(ty);
                let near = model::nearest(&m.key, &names);
                let hint = match near {
                    Some(n) if model::edit_distance(&m.key, n) <= 2 => format!(" Did you mean '{n}'?"),
                    _ => String::new(),
                };
                diags.add(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("components/{id}/{}", m.key)),
                    format!(
                        "unknown param '{}' for type {}.{hint} Allowed: {allowed} (`kerf schema {}`)",
                        m.key,
                        ty.name(),
                        ty.name()
                    ),
                );
            }
        }
        let mut comp = Comp::new(comps.len() as u32, id.to_string(), ty, item);
        comp.label = item.get("label").and_then(Value::str).map(String::from);
        comps.push(comp);
    }
    scene.comps = comps;
}

/// Stage 4: dependencies between components (`at.to`, `until`, `points` refs, `slope: "@id"`, `place.in`) and a topological order
/// of placement (Kahn, ties by document order). A cycle is reported (E_CYCLE) and its members are marked failed.
fn placement_order(scene: &mut Scene) -> Vec<usize> {
    let diags = scene.diags;
    let n = scene.comps.len();
    let mut deps: Vec<Vec<usize>> = Vec::with_capacity(n);
    let mut self_cycles: Vec<usize> = Vec::new();
    for (i, c) in scene.comps.iter().enumerate() {
        let mut names: Vec<&str> = Vec::new();
        collect_deps(c.node, &mut names);
        let mut idx: Vec<usize> = Vec::new();
        for nm in names {
            for (k, o) in scene.comps.iter().enumerate() {
                if o.id == nm && k != i && !idx.contains(&k) {
                    idx.push(k);
                }
            }
            if nm == c.id {
                diags.add(Level::Error, "E_CYCLE", Some(&c.id), Some(&format!("components/{}", c.id)), format!("placement cycle: '{}' refers to its own anchors. Give it an absolute position, e.g. \"at\": {{\"to\": [0, 0]}}", c.id));
                self_cycles.push(i);
            }
        }
        deps.push(idx);
    }
    for i in self_cycles {
        scene.comps[i].state = State::Failed;
    }
    let mut done = vec![false; n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    while order.len() < n {
        let picked = (0..n).find(|&i| !done[i] && deps[i].iter().all(|&d| done[d]));
        if let Some(p) = picked {
            done[p] = true;
            order.push(p);
            continue;
        }
        let start = (0..n).find(|&i| !done[i]).unwrap_or(0);
        let mut path: Vec<usize> = Vec::new();
        let mut cur = start;
        for _ in 0..=n {
            if let Some(at) = path.iter().position(|&x| x == cur) {
                let cyc = &path[at..];
                let mut txt = String::new();
                for &ci in cyc {
                    txt.push_str(&scene.comps[ci].id);
                    txt.push_str(" -> ");
                }
                txt.push_str(&scene.comps[cyc[0]].id);
                let first = &scene.comps[cyc[0]].id;
                diags.add_fix(
                    Level::Error,
                    "E_CYCLE",
                    Some(first),
                    Some(&format!("components/{first}/at")),
                    format!("placement cycle: {txt}. Components may not locate each other"),
                    "give one of them an absolute \"at\": {\"to\": [x, y]}".into(),
                );
                break;
            }
            path.push(cur);
            match deps[cur].iter().find(|&&d| !done[d]) {
                Some(&next) => cur = next,
                None => break,
            }
        }
        for (i, d) in done.iter_mut().enumerate() {
            if !*d {
                *d = true;
                scene.comps[i].state = State::Failed;
                order.push(i);
            }
        }
    }
    order
}

// ---- placement -----------------------------------------------------------------------------------------

struct AtSpec<'n> {
    anchor: &'n str,
    to: V2,
}

fn parse_at<'n>(scene: &Scene, comp_id: &str, default_anchor: &'static str, p: &mut Params<'n>) -> Option<AtSpec<'n>> {
    let Some(av) = p.raw("at") else { return Some(AtSpec { anchor: default_anchor, to: V2::init(0.0, 0.0) }) };
    if !matches!(av, Value::Object(_)) {
        p.fail(
            "at",
            "param 'at' must be {\"anchor\": \"bottom_left\", \"to\": \"comp@anchor\", \"offset\": [dx, dy]}".into(),
        );
        return None;
    }
    let mut spec = AtSpec { anchor: default_anchor, to: V2::init(0.0, 0.0) };
    if let Some(anc) = av.get("anchor") {
        if let Some(s) = anc.str() {
            spec.anchor = s
        } else {
            p.fail("at/anchor", "at.anchor must be an anchor name string".into());
            return None;
        }
    }
    let base = format!("components/{comp_id}/at/to");
    if let Some(to) = av.get("to").filter(|t| !t.is_null()) {
        match to {
            Value::String(s) => {
                if let Some(pt) = scene.resolve_ref_str(s, comp_id, &base) {
                    spec.to = pt
                } else {
                    p.ok = false;
                    return None;
                }
            }
            Value::Array(xy) => {
                let pt = (xy.len() == 2)
                    .then(|| Some(V2::init(units::parse_length(&xy[0])?, units::parse_length(&xy[1])?)))
                    .flatten();
                let Some(pt) = pt else {
                    p.fail(
                        "at/to",
                        "at.to must be a Ref, {\"ref\": ..., \"offset\": [dx, dy]} or a point [x, y]".into(),
                    );
                    return None;
                };
                spec.to = pt;
            }
            Value::Object(_) => {
                let Some(rs) = to.get("ref").and_then(Value::str) else {
                    p.fail("at/to", "at.to object needs a string \"ref\" (and optional \"offset\": [dx, dy])".into());
                    return None;
                };
                let Some(mut pt) = scene.resolve_ref_str(rs, comp_id, &base) else {
                    p.ok = false;
                    return None;
                };
                if let Some(off) = to.get("offset").filter(|o| !o.is_null()) {
                    pt = pt.add(p.offset_pair("at/to/offset", off)?);
                }
                spec.to = pt;
            }
            _ => {
                p.fail("at/to", "at.to must be a Ref string, {ref, offset} or [x, y]".into());
                return None;
            }
        }
    }
    if let Some(oa) = av.get("offset").and_then(Value::arr) {
        let off = (oa.len() == 2)
            .then(|| Some(V2::init(units::parse_length(&oa[0])?, units::parse_length(&oa[1])?)))
            .flatten();
        let Some(off) = off else {
            p.fail("at/offset", "at.offset must be [dx, dy] in inches".into());
            return None;
        };
        spec.to = spec.to.add(off);
    }
    Some(spec)
}

/// Rotation of a component: `rotate` plus `slope` (a rise:run, degrees, or "@id" to follow another component's slope). None
/// when the placement must stop (an unknown or unbuilt "@id", reported); a malformed `rotate`/`slope` is reported through `p`.
fn resolve_angle(scene: &Scene, comp_index: u32, comp_id: &str, p: &mut Params) -> Option<f64> {
    let mut angle = 0.0;
    if p.has("rotate") {
        angle += p.num("rotate", Some(0.0)).unwrap_or(0.0) * geom::RAD_PER_DEG;
    }
    if let Some(sv) = p.raw("slope") {
        match sv {
            Value::String(s) if s.starts_with('@') => {
                let ref_id = &s[1..];
                let Some(rc) = scene.find(ref_id) else {
                    let ids = scene.comp_ids();
                    let hint = model::nearest(ref_id, &ids).map_or(String::new(), |n| format!(" Did you mean \"@{n}\"?"));
                    scene.diags.add_fix(
                        Level::Error,
                        "E_REF_UNKNOWN",
                        Some(comp_id),
                        Some(&format!("components/{comp_id}/slope")),
                        format!("slope \"{s}\": no component '{ref_id}'.{hint} Components: {}", scene::join_ids(&ids)),
                        "use \"@<component id>\" of a truss (or any sloped member), or a literal \"4:12\"".into(),
                    );
                    return None;
                };
                if rc.state != State::Ok || rc.index == comp_index {
                    scene.diags.add_fix(
                        Level::Error,
                        "E_REF_UNKNOWN",
                        Some(comp_id),
                        Some(&format!("components/{comp_id}/slope")),
                        format!("slope \"{s}\": component '{ref_id}' did not build, so its slope is unavailable"),
                        "fix that component's errors first, or use a literal \"4:12\"".into(),
                    );
                    return None;
                }
                angle += rc.angle + rc.pitch_angle;
            }
            _ => match units::parse_slope(sv) {
                Some(r) => angle += r,
                None => p.fail("slope", format!("param 'slope' must be rise:run like \"4:12\", degrees, or \"@<component>\" to follow a truss/member (got {})", model::kind_or_text(sv))),
            },
        }
    }
    Some(angle)
}

/// z extent of a component's first instance: explicit `z` ([z0, z1] or a centre), else the natural thickness centred on the
/// section cut (or the middle of `run`), else the document `run`. None when `z` is malformed (reported).
fn resolve_z(scene: &Scene, p: &mut Params, built: &Built) -> Option<[f64; 2]> {
    let mut zbase = scene.run;
    let mut z_explicit = false;
    if let Some(zv) = p.raw("z") {
        match zv {
            Value::Array(za) => {
                let pair = (za.len() == 2)
                    .then(|| Some([units::parse_length(&za[0])?, units::parse_length(&za[1])?]))
                    .flatten();
                let Some(pair) = pair else {
                    p.fail("z", "param 'z' must be [z0, z1] or a single z (center)".into());
                    return None;
                };
                zbase = pair;
                z_explicit = true;
                if zbase[0] >= zbase[1] {
                    p.fail(
                        "z",
                        format!(
                            "z range must have z0 < z1 (got [{}, {}])",
                            model::num_text(zbase[0]),
                            model::num_text(zbase[1])
                        ),
                    );
                    return None;
                }
            }
            Value::Number(_) | Value::String(_) => {
                let Some(c) = units::parse_length(zv) else {
                    p.fail("z", "param 'z' must be [z0, z1] or a single z (center)".into());
                    return None;
                };
                let th = built.nat_z.unwrap_or(scene.run[1] - scene.run[0]);
                zbase = [c - th / 2.0, c + th / 2.0];
                z_explicit = true;
            }
            _ => {
                p.fail("z", "param 'z' must be [z0, z1] or a single z (center)".into());
                return None;
            }
        }
    }
    if !z_explicit && let Some(th) = built.nat_z {
        let mid = if built.abs_z { 0.0 } else { scene.default_z.unwrap_or((scene.run[0] + scene.run[1]) / 2.0) };
        zbase = [mid - th / 2.0, mid + th / 2.0];
    }
    Some(zbase)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
}

struct ArraySpec {
    axis: Axis,
    count: usize,
    spacing: f64,
}

/// `array`: {axis, count, spacing}. None when malformed (reported).
fn resolve_array(p: &mut Params) -> Option<ArraySpec> {
    let mut spec = ArraySpec { axis: Axis::X, count: 1, spacing: 0.0 };
    if let Some(av) = p.raw("array") {
        if !matches!(av, Value::Object(_)) {
            p.fail("array", "param 'array' must be {\"axis\": \"x|y|z\", \"count\": n, \"spacing\": s}".into());
            return None;
        }
        spec.axis = match p.field_choice("array", av, "axis", &["x", "y", "z"])? {
            "y" => Axis::Y,
            "z" => Axis::Z,
            _ => Axis::X,
        };
        let count = p.field_int("array", av, "count", 1, 1, 500)?;
        spec.count = num::to_int_clamped::<usize>(count as f64, 1, 500);
        let Some(sp) = av.get("spacing").and_then(units::parse_length) else {
            p.fail(
                "array/spacing",
                "array.spacing must be a length (inches between instances; may be negative)".into(),
            );
            return None;
        };
        spec.spacing = sp;
    }
    Some(spec)
}

struct Instances {
    xfs: Vec<Xf>,
    zs: Vec<[f64; 2]>,
}

/// Transform and z range of every instance (array expansion times cover-placed bars). None when the instance budget is exceeded
/// or the placement anchor is unknown (reported).
#[allow(clippy::too_many_arguments)]
fn instance_transforms(
    scene: &mut Scene,
    comp_id: &str,
    ty: &Entry,
    p: &mut Params,
    built: &Built,
    at: &AtSpec,
    angle: f64,
    zbase: [f64; 2],
    arr: &ArraySpec,
) -> Option<Instances> {
    let n_centers = built.centers.len().max(1);
    let n_inst = n_centers * arr.count;
    if scene.instances_total + n_inst > limits::MAX_INSTANCES_TOTAL {
        let what = format!(
            "instances after placing '{comp_id}' ({} already placed + {n_inst} from this component)",
            scene.instances_total
        );
        scene.diags.add_fix(
            Level::Error,
            "E_LIMIT",
            Some(comp_id),
            Some(&format!("components/{comp_id}")),
            limits::message(&what, scene.instances_total + n_inst, limits::MAX_INSTANCES_TOTAL, "Every array.count and place.count multiplies the cost of drawing and checking."),
            "draw one representative member and say the spacing in a note (e.g. 2X4 STUDS @ 16\" O.C.), or lower array.count / place.count".into(),
        );
        return None;
    }
    scene.instances_total += n_inst;
    let base_xf = if built.points_mode {
        Xf::translate(at.to.x, at.to.y).mul(Xf::rotate(angle))
    } else {
        let anc = model::box_anchor(built.box_, at.anchor)
            .or_else(|| built.anchors.iter().find(|n| n.name == at.anchor).map(|n| n.p));
        let Some(anc) = anc else {
            let mut names = model::BOX_ANCHOR_NAMES.join(", ");
            for n in &built.anchors {
                names.push_str(", ");
                names.push_str(&n.name);
            }
            p.fail_code(
                "E_ANCHOR_UNKNOWN",
                "at/anchor",
                format!("'{}' is not an anchor of '{comp_id}' ({}). Anchors: {names}", at.anchor, ty.name()),
            );
            return None;
        };
        Xf::translate(at.to.x, at.to.y).mul(Xf::rotate(angle)).mul(Xf::translate(-anc.x, -anc.y))
    };
    let mut xfs = Vec::with_capacity(n_inst);
    let mut zs = Vec::with_capacity(n_inst);
    for k in 0..arr.count {
        let kf = k as f64;
        for j in 0..n_centers {
            let mut xf = base_xf;
            if let Some(c) = built.centers.get(j) {
                xf = Xf::translate(c.x, c.y).mul(Xf::rotate(angle));
            }
            let mut zr = zbase;
            match arr.axis {
                Axis::X => xf = Xf::translate(kf * arr.spacing, 0.0).mul(xf),
                Axis::Y => xf = Xf::translate(0.0, kf * arr.spacing).mul(xf),
                Axis::Z => zr = [zbase[0] + kf * arr.spacing, zbase[1] + kf * arr.spacing],
            }
            xfs.push(xf);
            zs.push(zr);
        }
    }
    Some(Instances { xfs, zs })
}

/// The prisms of every instance in world coordinates, in builder order per instance.
fn world_prisms(
    style: &Style,
    comp_index: u32,
    built: &Built,
    inst: &Instances,
    emb_override: Option<bool>,
    dashed: bool,
) -> Vec<Prism> {
    let mut world = Vec::with_capacity(inst.xfs.len() * built.prisms.len());
    for (ii, (xf, z)) in inst.xfs.iter().zip(&inst.zs).enumerate() {
        for pr in &built.prisms {
            let mut q = pr.transform(xf);
            q.comp = comp_index;
            q.role = style.role_of(&q.material);
            q.instance = ii as u32;
            q.z0 = z[0];
            q.z1 = z[1];
            if pr.zhalf > 0.0 || pr.zoff != 0.0 {
                let zc = (z[0] + z[1]) / 2.0 + pr.zoff;
                let half = if pr.zhalf > 0.0 { pr.zhalf } else { (z[1] - z[0]) / 2.0 };
                q.z0 = zc - half;
                q.z1 = zc + half;
            }
            for c in &mut q.path3 {
                c[2] += (z[0] + z[1]) / 2.0;
            }
            if let Some(e) = emb_override {
                q.embedded = e;
            }
            if dashed {
                q.kind = model::Kind::Ghost;
                q.pen = Some(crate::pen::Pen::Hidden);
                q.dashed = true;
            }
            world.push(q);
        }
    }
    world
}

fn place_component(scene: &mut Scene, idx: usize) {
    let (id, ty, node, index) = {
        let c = &scene.comps[idx];
        (c.id.clone(), c.ty, c.node, c.index)
    };
    let diags = scene.diags;
    let mut p = Params::new(diags, node, &id, "components", ty.name());
    scene.comps[idx].state = State::Failed;

    let Some(at) = parse_at(scene, &id, ty.traits.default_anchor, &mut p) else { return };
    scene.comps[idx].place_pt = at.to;
    let Some(angle) = resolve_angle(scene, index, &id, &mut p) else { return };
    let mirror = p.boolean("mirror", false);
    let visible = p.boolean("visible", true);
    let shown = p.choice("shown", Some("solid"), &["solid", "hidden", "where_occurs", "dashed"]);
    if !p.ok {
        return;
    }
    let dashed = shown != Some("solid");
    let where_occurs = matches!(shown, Some("where_occurs" | "dashed"));
    if shown == Some("dashed") {
        diags.add_fix(
            Level::Warning,
            "W_SHOWN_DASHED",
            Some(&id),
            Some(&format!("components/{id}/shown")),
            format!("'{id}': shown \"dashed\" is the old name of \"where_occurs\" (dashed, and notes that target it end in (WHERE OCCURS)); a member that is only hidden behind or below others wants \"hidden\" (dashed, no suffix)"),
            format!("{{\"op\":\"update\",\"path\":\"components/{id}\",\"value\":{{\"shown\":\"where_occurs\"}}}} or {{\"op\":\"update\",\"path\":\"components/{id}\",\"value\":{{\"shown\":\"hidden\"}}}}"),
        );
    }

    let built = {
        let mut ctx = Ctx { scene, style: scene.style, p, origin: at.to, angle };
        let b = builders::build(ty.ty, &mut ctx);
        p = ctx.p;
        b
    };
    let Some(mut built) = built else { return };
    if !p.ok {
        return;
    }
    if mirror && !built.points_mode {
        built = builders::mirror_about_center(&built);
    }

    let Some(zbase) = resolve_z(scene, &mut p, &built) else { return };
    let Some(arr) = resolve_array(&mut p) else { return };
    let emb_override = if p.has("embedded") { Some(p.boolean("embedded", false)) } else { None };
    if !p.ok {
        return;
    }

    let Some(inst) = instance_transforms(scene, &id, ty, &mut p, &built, &at, angle, zbase, &arr) else { return };
    let world = world_prisms(scene.style, index, &built, &inst, emb_override, dashed);
    let pitch_angle = if ty.traits.has_pitch { built.pitch } else { 0.0 };
    let comp = &mut scene.comps[idx];
    comp.angle = angle;
    comp.pitch_angle = pitch_angle;
    comp.embedded = emb_override.unwrap_or_else(|| built.prisms.first().is_some_and(|p| p.embedded));
    comp.built = built;
    comp.xfs = inst.xfs;
    comp.zs = inst.zs;
    comp.world = world;
    comp.visible = visible;
    comp.dashed = dashed;
    comp.where_occurs = where_occurs;
    comp.arr_count = arr.count;
    comp.state = State::Ok;
}

/// Visible world prisms of the components a view draws (SPEC 25.4: `views`, `only`, `omit`).
pub fn view_prisms(scene: &Scene, members: &Members) -> Vec<Prism> {
    scene
        .comps
        .iter()
        .filter(|c| c.state == State::Ok && c.visible && members.draws(c))
        .flat_map(|c| c.world.iter().cloned())
        .collect()
}
