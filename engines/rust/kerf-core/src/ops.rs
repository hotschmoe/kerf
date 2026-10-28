//! Document edit ops (SPEC 14): add / update / remove / set, atomic, with citation rules.

use crate::compile;
use crate::json::{self, Member, Value};
use crate::model::{self, Diags, Level};
use crate::scene;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Actor {
    Llm,
    Designer,
}

pub struct Applied {
    pub doc: Value,
    pub changed: Vec<String>,
}

struct Ctx<'d> {
    diags: &'d Diags,
    actor: Actor,
    doc: Value,
    changed: Vec<String>,
    op_index: usize,
    ok: bool,
}

impl Ctx<'_> {
    fn fail(&mut self, code: &'static str, path: &str, msg: String) {
        self.ok = false;
        self.diags.add(Level::Error, code, None, Some(path), format!("op {}: {}", self.op_index, msg));
    }

    fn touch(&mut self, id: &str) {
        if !self.changed.iter().any(|c| c == id) {
            self.changed.push(id.to_string());
        }
    }

    fn list(&self, key: &str) -> Vec<Value> {
        self.doc.get(key).and_then(Value::arr).map(<[Value]>::to_vec).unwrap_or_default()
    }

    fn set_list(&mut self, key: &str, list: Vec<Value>) {
        set_key(&mut self.doc, key, Value::Array(list));
    }
}

fn set_key(obj: &mut Value, key: &str, val: Value) {
    if !matches!(obj, Value::Object(_)) {
        *obj = Value::Object(Vec::new());
    }
    if let Value::Object(ms) = obj {
        match ms.iter_mut().find(|m| m.key == key) {
            Some(m) => m.value = val,
            None => ms.push(Member { key: key.to_string(), value: val }),
        }
    }
}

fn with_key(mut obj: Value, key: &str, val: Value) -> Value {
    set_key(&mut obj, key, val);
    obj
}

fn id_of(v: &Value) -> Option<&str> {
    v.get("id").and_then(Value::str)
}

fn index_of_id(list: &[Value], id: &str) -> Option<usize> {
    list.iter().position(|v| id_of(v) == Some(id))
}

fn unknown_id(c: &mut Ctx, kind: &str, id: &str, list: &[Value], path: &str) {
    let ids: Vec<&str> = list.iter().filter_map(id_of).collect();
    let msg = match model::nearest(id, &ids) {
        Some(n) => format!("no {kind} '{id}'; did you mean '{n}'? {kind}s: {}", scene::join_ids(&ids)),
        None => format!("no {kind} '{id}'. {kind}s: {}", scene::join_ids(&ids)),
    };
    c.fail("E_REF_UNKNOWN", path, msg);
}

fn sanitize_cites(c: &Ctx, ann: &Value, force_reset: bool) -> Value {
    let Some(arr) = ann.get("cite").and_then(Value::arr) else { return ann.clone() };
    let mut out = Vec::with_capacity(arr.len());
    for ci in arr {
        let st = ci.get("status").map_or("suggested", |s| s.str().unwrap_or("suggested"));
        let verified = st == "verified";
        let suggested = || Value::String("suggested".to_string());
        if c.actor == Actor::Llm && (verified || force_reset) {
            if verified {
                let sec = ci.get("section").map_or("", |s| s.str().unwrap_or(""));
                let code = ci.get("code").map_or("", |s| s.str().unwrap_or(""));
                c.diags.add(
                    Level::Info,
                    "I_CITE_DOWNGRADED",
                    id_of(ann),
                    None,
                    format!("citation {code} {sec} on '{}' was verified; only the designer can verify code citations, so it was reset to suggested", id_of(ann).unwrap_or("")),
                );
            }
            out.push(with_key(ci.clone(), "status", suggested()));
        } else if ci.get("status").is_none() {
            out.push(with_key(ci.clone(), "status", suggested()));
        } else {
            out.push(ci.clone());
        }
    }
    with_key(ann.clone(), "cite", Value::Array(out))
}

fn references_comp(v: &Value, id: &str) -> bool {
    match v {
        Value::String(s) => {
            let mut head = s.split('@').next().unwrap_or("");
            if let Some(d) = head.find('.') {
                head = &head[..d];
            }
            if let Some(h) = head.find('#') {
                head = &head[..h];
            }
            head == id
        }
        Value::Object(_) => v.get("ref").is_some_and(|r| references_comp(r, id)),
        _ => false,
    }
}

fn annotation_references(ann: &Value, id: &str) -> bool {
    if let Some(s) = ann.get("target").and_then(Value::str) {
        if s.split('.').next() == Some(id) {
            return true;
        }
    }
    ["at", "from", "to"].iter().any(|k| ann.get(k).is_some_and(|x| references_comp(x, id)))
}

fn dependents(c: &Ctx, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for comp in c.list("components") {
        let Some(cid) = id_of(&comp) else { continue };
        if cid == id {
            continue;
        }
        let mut deps = Vec::new();
        compile::collect_deps(&comp, &mut deps);
        if deps.contains(&id) {
            out.push(cid.to_string());
        }
    }
    for view in c.list("views") {
        let vid = id_of(&view).unwrap_or("?");
        if let Some(aa) = view.get("annotations").and_then(Value::arr) {
            for ann in aa {
                if annotation_references(ann, id) {
                    out.push(format!("views/{vid}/annotations/{}", id_of(ann).unwrap_or("?")));
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Add,
    Update,
    Remove,
    Set,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Add => "add",
            Kind::Update => "update",
            Kind::Remove => "remove",
            Kind::Set => "set",
        }
    }
}

struct Op<'v> {
    kind: Kind,
    path: &'v str,
    seg: Vec<&'v str>,
    value: Option<&'v Value>,
    raw: &'v Value,
}

fn apply_one(c: &mut Ctx, raw: &Value) {
    if !matches!(raw, Value::Object(_)) {
        return c.fail("E_OP", "", format!("each op must be an object like {{\"op\": \"add\", \"path\": \"components\", \"value\": {{...}}}}; got a {}", raw.kind_name()));
    }
    let Some(kind_s) = raw.get("op").and_then(Value::str) else {
        return c.fail("E_OP", "", "op needs \"op\": add | update | remove | set".to_string());
    };
    let Some(path) = raw.get("path").and_then(Value::str) else {
        return c.fail("E_OP", "", "op needs a string \"path\" such as \"components\", \"components/<id>\", \"views/<id>/annotations\"".to_string());
    };
    let value = raw.get("value");
    let kind = match kind_s {
        "add" => Kind::Add,
        "update" => Kind::Update,
        "remove" => Kind::Remove,
        "set" => Kind::Set,
        _ => return c.fail("E_OP", path, format!("unknown op \"{kind_s}\"; use add, update, remove or set")),
    };
    if kind != Kind::Remove && value.is_none_or(Value::is_null) {
        return c.fail("E_OP", path, format!("op {kind_s} {path} needs a \"value\""));
    }
    let seg: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let Some(&root) = seg.first() else { return c.fail("E_OP", path, "empty path".to_string()) };
    let op = Op { kind, path, seg, value, raw };
    if root == "doc" {
        return apply_doc(c, &op);
    }
    if !matches!(c.doc, Value::Object(_)) {
        c.doc = Value::Object(Vec::new());
    }
    match root {
        "meta" => apply_meta(c, &op),
        "components" => apply_components(c, &op),
        "views" => apply_views(c, &op),
        _ => c.fail("E_OP", path, format!("unknown path root \"{root}\"; use doc, meta, components, views")),
    }
}

fn sanitize_annotations(c: &Ctx, view: &Value) -> Value {
    match view.get("annotations").and_then(Value::arr) {
        Some(aa) => with_key(view.clone(), "annotations", Value::Array(aa.iter().map(|a| sanitize_cites(c, a, false)).collect())),
        None => view.clone(),
    }
}

fn apply_doc(c: &mut Ctx, op: &Op) {
    if op.kind != Kind::Set || op.seg.len() != 1 {
        return c.fail("E_OP", op.path, "only {\"op\": \"set\", \"path\": \"doc\", \"value\": {whole document}} may target \"doc\"".to_string());
    }
    let Some(v) = op.value.filter(|v| matches!(v, Value::Object(_))) else {
        return c.fail("E_OP", op.path, "set doc needs the whole document object as value".to_string());
    };
    let mut v = v.clone();
    if c.actor == Actor::Llm {
        if let Some(va) = v.get("views").and_then(Value::arr) {
            let nv: Vec<Value> = va.iter().map(|view| sanitize_annotations(c, view)).collect();
            set_key(&mut v, "views", Value::Array(nv));
        }
    }
    c.doc = v;
    c.changed.clear();
    let mut ids = Vec::new();
    for key in ["components", "views"] {
        ids.extend(c.list(key).iter().filter_map(|x| id_of(x).map(str::to_string)));
    }
    for i in ids {
        c.touch(&i);
    }
}

fn apply_meta(c: &mut Ctx, op: &Op) {
    if op.seg.len() != 1 || matches!(op.kind, Kind::Add | Kind::Remove) {
        return c.fail("E_OP", op.path, "meta supports only {\"op\": \"update\", \"path\": \"meta\", \"value\": {merge patch}}".to_string());
    }
    let Some(value) = op.value else { return };
    let nv = if op.kind == Kind::Set {
        value.clone()
    } else {
        let empty = Value::Object(Vec::new());
        json::merge_patch(c.doc.get("meta").unwrap_or(&empty), value)
    };
    set_key(&mut c.doc, "meta", nv);
    c.touch("meta");
}

fn id_change(patch: &Value, id: &str) -> Option<String> {
    patch.get("id").and_then(Value::str).filter(|s| *s != id).map(str::to_string)
}

fn apply_components(c: &mut Ctx, op: &Op) {
    let value = op.value.unwrap_or(&Value::Null);
    let mut list = c.list("components");
    if op.seg.len() == 1 && op.kind == Kind::Add {
        let Some(id) = id_of(value).map(str::to_string) else {
            return c.fail("E_PARAM", op.path, "a component needs a string \"id\" matching [a-z][a-z0-9_]*".to_string());
        };
        if index_of_id(&list, &id).is_some() {
            return c.fail("E_DUP_ID", op.path, format!("component id '{id}' already exists; ids must be unique (use op update to change it)"));
        }
        let mut at = list.len();
        if let Some(bid) = op.raw.get("before").and_then(Value::str) {
            match index_of_id(&list, bid) {
                Some(i) => at = i,
                None => return unknown_id(c, "component", bid, &list, op.path),
            }
        }
        list.insert(at, value.clone());
        c.set_list("components", list);
        c.touch(&id);
        return;
    }
    if op.seg.len() == 2 {
        let id = op.seg[1];
        let Some(idx) = index_of_id(&list, id) else { return unknown_id(c, "component", id, &list, op.path) };
        match op.kind {
            Kind::Update | Kind::Set => {
                if let Some(s) = id_change(value, id) {
                    return c.fail("E_OP", op.path, format!("ids cannot be changed by update (got \"{s}\" for '{id}'); remove and re-add the component, then fix references"));
                }
                list[idx] = if op.kind == Kind::Set { value.clone() } else { json::merge_patch(&list[idx], value) };
                c.set_list("components", list);
                c.touch(id);
                return;
            }
            Kind::Remove => {
                let deps = dependents(c, id);
                if !deps.is_empty() {
                    return c.fail("E_REF_UNKNOWN", op.path, format!("cannot remove '{id}': still referenced by {}. Remove or re-point those first (or remove them in the same op batch before this op)", scene::join_ids(&deps)));
                }
                list.remove(idx);
                c.set_list("components", list);
                c.touch(id);
                return;
            }
            Kind::Add => {}
        }
    }
    c.fail("E_OP", op.path, format!("unsupported op {} on path {}; components paths: \"components\" (add), \"components/<id>\" (update, remove)", op.kind.name(), op.path));
}

fn apply_views(c: &mut Ctx, op: &Op) {
    let value = op.value.unwrap_or(&Value::Null);
    let mut list = c.list("views");
    if op.seg.len() == 1 && op.kind == Kind::Add {
        let Some(id) = id_of(value).map(str::to_string) else {
            return c.fail("E_PARAM", op.path, "a view needs a string \"id\"".to_string());
        };
        if index_of_id(&list, &id).is_some() {
            return c.fail("E_DUP_ID", op.path, format!("view id '{id}' already exists"));
        }
        let nv = if c.actor == Actor::Llm { sanitize_annotations(c, value) } else { value.clone() };
        list.push(nv);
        c.set_list("views", list);
        c.touch(&id);
        return;
    }
    if op.seg.len() >= 2 {
        let vid = op.seg[1];
        let Some(vi) = index_of_id(&list, vid) else { return unknown_id(c, "view", vid, &list, op.path) };
        if op.seg.len() == 2 {
            if op.kind == Kind::Update {
                if value.get("annotations").is_some() {
                    return c.fail("E_OP", op.path, format!("update views/{vid} cannot carry \"annotations\"; use add/update/remove on views/{vid}/annotations[/<id>]"));
                }
                if id_change(value, vid).is_some() {
                    return c.fail("E_OP", op.path, "view ids cannot be changed by update".to_string());
                }
                list[vi] = json::merge_patch(&list[vi], value);
                c.set_list("views", list);
                c.touch(vid);
                return;
            }
            if op.kind == Kind::Remove {
                list.remove(vi);
                c.set_list("views", list);
                c.touch(vid);
                return;
            }
        }
        if op.seg.len() >= 3 && op.seg[2] == "annotations" {
            let mut anns: Vec<Value> = list[vi].get("annotations").and_then(Value::arr).map(<[Value]>::to_vec).unwrap_or_default();
            if op.seg.len() == 3 && op.kind == Kind::Add {
                let Some(aid) = id_of(value).map(str::to_string) else {
                    return c.fail("E_PARAM", op.path, "an annotation needs a string \"id\"".to_string());
                };
                if index_of_id(&anns, &aid).is_some() {
                    return c.fail("E_DUP_ID", op.path, format!("annotation id '{aid}' already exists in view '{vid}'"));
                }
                anns.push(sanitize_cites(c, value, false));
                c.touch(&aid);
            } else if op.seg.len() == 4 {
                let aid = op.seg[3];
                let Some(ai) = index_of_id(&anns, aid) else { return unknown_id(c, "annotation", aid, &anns, op.path) };
                match op.kind {
                    Kind::Update => {
                        if id_change(value, aid).is_some() {
                            return c.fail("E_OP", op.path, "annotation ids cannot be changed by update".to_string());
                        }
                        let merged = json::merge_patch(&anns[ai], value);
                        let reset = value.get("text").is_some() || value.get("cite").is_some();
                        anns[ai] = sanitize_cites(c, &merged, reset && c.actor == Actor::Llm);
                    }
                    Kind::Remove => {
                        anns.remove(ai);
                    }
                    _ => return c.fail("E_OP", op.path, format!("unsupported op {} on annotation path", op.kind.name())),
                }
                c.touch(aid);
            } else {
                return c.fail("E_OP", op.path, "annotation paths: \"views/<id>/annotations\" (add), \"views/<id>/annotations/<id>\" (update, remove)".to_string());
            }
            set_key(&mut list[vi], "annotations", Value::Array(anns));
            c.set_list("views", list);
            return;
        }
    }
    c.fail("E_OP", op.path, format!("unsupported op {} on path {}; view paths: \"views\" (add), \"views/<id>\" (update, remove), \"views/<id>/annotations[/<id>]\"", op.kind.name(), op.path));
}

const OPS_SHAPES: &str = "an array of op objects [{\"op\":...}, ...], a single op object {\"op\":...}, or {\"ops\":[...], \"why\":\"...\"}";

pub struct Normalized {
    pub ops: Vec<Value>,
    pub why: Option<String>,
}

fn describe_input(v: &Value) -> String {
    let Value::Object(ms) = v else { return format!("a {}", v.kind_name()) };
    let mut out = String::from("an object with keys ");
    if ms.is_empty() {
        out.push_str("(none)");
    }
    for (i, m) in ms.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&m.key);
    }
    out
}

pub fn normalize(input: &Value, diags: &Diags) -> Option<Normalized> {
    let why = || input.get("why").and_then(Value::str).map(str::to_string);
    match input {
        Value::Array(a) => return Some(Normalized { ops: a.clone(), why: None }),
        Value::Object(_) => {
            if let Some(inner) = input.get("ops") {
                if let Value::Array(a) = inner {
                    return Some(Normalized { ops: a.clone(), why: why() });
                }
                if matches!(inner, Value::Object(_)) && inner.get("op").is_some() {
                    return Some(Normalized { ops: alloc::vec![inner.clone()], why: why() });
                }
            } else if input.get("op").is_some() {
                return Some(Normalized { ops: alloc::vec![input.clone()], why: None });
            }
        }
        _ => {}
    }
    let fix = if matches!(input, Value::Object(_)) { "wrap a single op in [ ], or put the ops under an \"ops\" key" } else { "pass the op list as a JSON array" };
    diags.add_fix(Level::Error, "E_PARAM", None, Some("ops"), format!("the ops input must be {OPS_SHAPES}; got {}", describe_input(input)), fix.to_string());
    None
}

pub fn apply(doc: &Value, ops_input: &Value, actor: Actor, diags: &Diags) -> Option<Applied> {
    let norm = normalize(ops_input, diags)?;
    let mut c = Ctx { diags, actor, doc: doc.clone(), changed: Vec::new(), op_index: 0, ok: true };
    for (i, op) in norm.ops.iter().enumerate() {
        c.op_index = i;
        apply_one(&mut c, op);
        if !c.ok {
            return None;
        }
    }
    Some(Applied { doc: c.doc, changed: c.changed })
}
