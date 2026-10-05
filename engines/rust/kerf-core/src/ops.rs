//! `apply` ops (SPEC section 14): atomic document edits with merge patch semantics.

use crate::diag::{Diag, Level, nearest};
use crate::json::{self, obj};
use crate::resolve::parse_ref;
use serde_json::{Map, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Actor {
    Llm,
    Designer,
}

pub struct Applied {
    pub doc: Value,
    pub changed: Vec<String>,
    pub diags: Vec<Diag>,
}

/// RFC 7396 merge patch.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(p) => {
            if !target.is_object() {
                *target = Value::Object(Map::new());
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.shift_remove(k);
                } else {
                    let entry = t.entry(k.clone()).or_insert(Value::Null);
                    merge_patch(entry, v);
                }
            }
        }
        other => *target = other.clone(),
    }
}

fn perr(path: &str, msg: String, fix: Option<String>) -> Diag {
    let mut d = Diag::error("E_PARAM", msg).path(path.to_string());
    if let Some(f) = fix {
        d = d.fix(f);
    }
    d
}

const PATH_HELP: &str = "valid paths: doc | meta | components | components/<id> | views | views/<id> | views/<id>/annotations | views/<id>/annotations/<id>";

fn find_idx(arr: &[Value], id: &str) -> Option<usize> {
    arr.iter().position(|x| x.get("id").and_then(|i| i.as_str()) == Some(id))
}

fn ids_of(arr: &[Value]) -> Vec<String> {
    arr.iter().filter_map(|x| x.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())).collect()
}

fn unknown_id(kind: &str, id: &str, ids: &[String], path: &str) -> Diag {
    let mut d = Diag::error("E_REF_UNKNOWN", format!("{} \"{}\" does not exist.", kind, id)).path(path.to_string());
    d = d.fix(match nearest(id, ids.iter().map(|s| s.as_str())) {
        Some(n) => format!("did you mean \"{}\"? existing: {}", n, ids.join(", ")),
        None => format!("existing: {}", if ids.is_empty() { "(none)".to_string() } else { ids.join(", ") }),
    });
    d
}

/// Component ids referenced by a JSON value (Ref strings / {ref} objects, recursively).
fn refs_in(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            if s.contains('@') {
                if let Ok(r) = parse_ref(s) {
                    if let Some(c) = r.comp {
                        out.push(c);
                    }
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| refs_in(x, out)),
        Value::Object(m) => m.values().for_each(|x| refs_in(x, out)),
        _ => {}
    }
}

fn dependents_of(doc: &Value, id: &str) -> Vec<String> {
    let mut out = vec![];
    if let Some(comps) = doc.get("components").and_then(|c| c.as_array()) {
        for c in comps {
            let cid = c.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if cid == id {
                continue;
            }
            let mut refs = vec![];
            for key in ["at", "points", "profile", "place"] {
                if let Some(v) = c.get(key) {
                    refs_in(v, &mut refs);
                }
            }
            if let Some(p) = c.get("place").and_then(|p| p.get("in")).and_then(|i| i.as_str()) {
                refs.push(p.split(['.', '#']).next().unwrap_or(p).to_string());
            }
            if refs.iter().any(|r| r == id) {
                out.push(format!("components/{}", cid));
            }
        }
    }
    if let Some(views) = doc.get("views").and_then(|v| v.as_array()) {
        for v in views {
            let vid = v.get("id").and_then(|i| i.as_str()).unwrap_or("");
            for an in v.get("annotations").and_then(|a| a.as_array()).cloned().unwrap_or_default() {
                let aid = an.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let mut refs = vec![];
                for key in ["at", "from", "to"] {
                    if let Some(x) = an.get(key) {
                        refs_in(x, &mut refs);
                    }
                }
                if let Some(t) = an.get("target").and_then(|t| t.as_str()) {
                    refs.push(t.split(['.', '#']).next().unwrap_or(t).to_string());
                }
                if refs.iter().any(|r| r == id) {
                    out.push(format!("views/{}/annotations/{}", vid, aid));
                }
            }
        }
    }
    out
}

fn downgrade_cites(note: &mut Value, actor: Actor, reset_all: bool, label: &str, diags: &mut Vec<Diag>) {
    if actor != Actor::Llm {
        return;
    }
    if let Some(cites) = note.get_mut("cite").and_then(|c| c.as_array_mut()) {
        for c in cites {
            let verified = c.get("status").and_then(|s| s.as_str()) == Some("verified");
            if verified || reset_all {
                if let Some(m) = c.as_object_mut() {
                    if verified {
                        diags.push(
                            Diag::info(
                                "I_CITE_DOWNGRADED",
                                format!("{}: citation {} {} was set to \"verified\" by the LLM or an LLM edit changed the note; it is now \"suggested\". Only the designer can verify citations.", label,
                                    m.get("code").and_then(|x| x.as_str()).unwrap_or(""), m.get("section").and_then(|x| x.as_str()).unwrap_or("")),
                            ),
                        );
                    }
                    m.insert("status".into(), Value::String("suggested".into()));
                }
            }
        }
    }
}

fn downgrade_doc(doc: &mut Value, actor: Actor, diags: &mut Vec<Diag>) {
    if actor != Actor::Llm {
        return;
    }
    if let Some(views) = doc.get_mut("views").and_then(|v| v.as_array_mut()) {
        for v in views {
            let vid = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
            if let Some(anns) = v.get_mut("annotations").and_then(|a| a.as_array_mut()) {
                for a in anns {
                    let aid = a.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                    downgrade_cites(a, actor, false, &format!("views/{}/annotations/{}", vid, aid), diags);
                }
            }
        }
    }
}

fn touch(changed: &mut Vec<String>, id: &str) {
    if !changed.iter().any(|c| c == id) {
        changed.push(id.to_string());
    }
}

/// Apply all ops to a copy of `doc`. Returns Err(diags) on the first failing op.
pub fn apply_ops(doc: &Value, ops: &Value, actor: Actor) -> Result<Applied, Vec<Diag>> {
    let Some(list) = ops.as_array() else {
        return Err(vec![perr("ops", "ops must be an array of {op, path, value} objects".into(), Some("send {\"op\":\"set\",\"path\":\"doc\",\"value\":{...}} for a first build".into()))]);
    };
    let mut d = doc.clone();
    let mut changed: Vec<String> = vec![];
    let mut diags: Vec<Diag> = vec![];
    for (k, op) in list.iter().enumerate() {
        let opn = op.get("op").and_then(|x| x.as_str()).unwrap_or("");
        let path = op.get("path").and_then(|x| x.as_str()).unwrap_or("");
        let at = format!("ops/{}", k);
        let value = op.get("value");
        let before = op.get("before").and_then(|b| b.as_str());
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let need_value = |name: &str| -> Result<&Value, Vec<Diag>> {
            value.ok_or_else(|| vec![perr(&at, format!("{} {}: \"value\" is required", name, path), None)])
        };
        match (opn, parts.as_slice()) {
            ("set", ["doc"]) => {
                let v = need_value("set")?;
                if !v.is_object() {
                    return Err(vec![perr(&at, "set doc: value must be the whole Kerf document object".into(), None)]);
                }
                d = v.clone();
                downgrade_doc(&mut d, actor, &mut diags);
                touch(&mut changed, "doc");
            }
            ("update", ["meta"]) => {
                let v = need_value("update")?;
                let m = d.as_object_mut().ok_or_else(|| vec![perr(&at, "document is not an object".into(), None)])?;
                let entry = m.entry("meta".to_string()).or_insert(Value::Object(Map::new()));
                merge_patch(entry, v);
                touch(&mut changed, "meta");
            }
            ("add", ["components"]) => {
                let v = need_value("add")?;
                let id = v.get("id").and_then(|i| i.as_str()).ok_or_else(|| vec![perr(&at, "add components: value needs an \"id\" ([a-z][a-z0-9_]*)".into(), None)])?.to_string();
                let comps = d.as_object_mut().unwrap().entry("components".to_string()).or_insert(Value::Array(vec![])).as_array_mut().ok_or_else(|| vec![perr(&at, "components is not an array".into(), None)])?;
                if find_idx(comps, &id).is_some() {
                    return Err(vec![Diag::error("E_DUP_ID", format!("component \"{}\" already exists.", id)).id(&id).fix("use op update to change it, or pick a new id")]);
                }
                match before {
                    Some(b) => {
                        let i = find_idx(comps, b).ok_or_else(|| vec![unknown_id("component", b, &ids_of(comps), &format!("{}/before", at))])?;
                        comps.insert(i, v.clone());
                    }
                    None => comps.push(v.clone()),
                }
                touch(&mut changed, &id);
            }
            ("update", ["components", id]) => {
                let v = need_value("update")?;
                let comps = d.get_mut("components").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("component", id, &[], path)])?;
                let i = match find_idx(comps, id) {
                    Some(i) => i,
                    None => return Err(vec![unknown_id("component", id, &ids_of(comps), path)]),
                };
                if v.get("id").and_then(|x| x.as_str()).map_or(false, |n| n != *id) {
                    return Err(vec![perr(path, "update cannot change a component id (it would break references); remove and add instead".into(), None)]);
                }
                merge_patch(&mut comps[i], v);
                touch(&mut changed, id);
            }
            ("remove", ["components", id]) => {
                let deps = dependents_of(&d, id);
                if !deps.is_empty() {
                    return Err(vec![Diag::error(
                        "E_REF_UNKNOWN",
                        format!("cannot remove \"{}\": still referenced by {}.", id, deps.join(", ")),
                    )
                    .id(*id)
                    .path(path.to_string())
                    .fix("update or remove the dependents in the same ops batch (remove them first)")]);
                }
                let comps = d.get_mut("components").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("component", id, &[], path)])?;
                match find_idx(comps, id) {
                    Some(i) => {
                        comps.remove(i);
                    }
                    None => return Err(vec![unknown_id("component", id, &ids_of(comps), path)]),
                }
                touch(&mut changed, id);
            }
            ("add", ["views"]) => {
                let v = need_value("add")?;
                let id = v.get("id").and_then(|i| i.as_str()).ok_or_else(|| vec![perr(&at, "add views: value needs an \"id\"".into(), None)])?.to_string();
                let views = d.as_object_mut().unwrap().entry("views".to_string()).or_insert(Value::Array(vec![])).as_array_mut().ok_or_else(|| vec![perr(&at, "views is not an array".into(), None)])?;
                if find_idx(views, &id).is_some() {
                    return Err(vec![Diag::error("E_DUP_ID", format!("view \"{}\" already exists.", id)).id(&id)]);
                }
                let mut nv = v.clone();
                if let Some(anns) = nv.get_mut("annotations").and_then(|a| a.as_array_mut()) {
                    for a in anns {
                        downgrade_cites(a, actor, false, &format!("views/{}/annotations", id), &mut diags);
                    }
                }
                views.push(nv);
                touch(&mut changed, &id);
            }
            ("update", ["views", vid]) => {
                let v = need_value("update")?;
                if v.get("annotations").is_some() {
                    return Err(vec![perr(path, "update views/<id>: annotations are edited with views/<id>/annotations ops, not in the view patch".into(), None)]);
                }
                let views = d.get_mut("views").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("view", vid, &[], path)])?;
                let i = match find_idx(views, vid) {
                    Some(i) => i,
                    None => return Err(vec![unknown_id("view", vid, &ids_of(views), path)]),
                };
                merge_patch(&mut views[i], v);
                touch(&mut changed, vid);
            }
            ("remove", ["views", vid]) => {
                let views = d.get_mut("views").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("view", vid, &[], path)])?;
                match find_idx(views, vid) {
                    Some(i) => {
                        views.remove(i);
                    }
                    None => return Err(vec![unknown_id("view", vid, &ids_of(views), path)]),
                }
                touch(&mut changed, vid);
            }
            ("add", ["views", vid, "annotations"]) => {
                let v = need_value("add")?;
                let id = v.get("id").and_then(|i| i.as_str()).ok_or_else(|| vec![perr(&at, "add annotation: value needs an \"id\"".into(), None)])?.to_string();
                let views = d.get_mut("views").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("view", vid, &[], path)])?;
                let vi = match find_idx(views, vid) {
                    Some(i) => i,
                    None => return Err(vec![unknown_id("view", vid, &ids_of(views), path)]),
                };
                let view = views[vi].as_object_mut().unwrap();
                let anns = view.entry("annotations".to_string()).or_insert(Value::Array(vec![])).as_array_mut().ok_or_else(|| vec![perr(&at, "annotations is not an array".into(), None)])?;
                if find_idx(anns, &id).is_some() {
                    return Err(vec![Diag::error("E_DUP_ID", format!("annotation \"{}\" already exists in view {}.", id, vid)).id(&id).fix("use op update to change it")]);
                }
                let mut nv = v.clone();
                downgrade_cites(&mut nv, actor, false, &format!("views/{}/annotations/{}", vid, id), &mut diags);
                match before {
                    Some(b) => {
                        let i = find_idx(anns, b).ok_or_else(|| vec![unknown_id("annotation", b, &ids_of(anns), &format!("{}/before", at))])?;
                        anns.insert(i, nv);
                    }
                    None => anns.push(nv),
                }
                touch(&mut changed, &id);
            }
            ("update", ["views", vid, "annotations", aid]) => {
                let v = need_value("update")?;
                let views = d.get_mut("views").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("view", vid, &[], path)])?;
                let vi = match find_idx(views, vid) {
                    Some(i) => i,
                    None => return Err(vec![unknown_id("view", vid, &ids_of(views), path)]),
                };
                let anns = views[vi].get_mut("annotations").and_then(|a| a.as_array_mut()).ok_or_else(|| vec![unknown_id("annotation", aid, &[], path)])?;
                let ai = match find_idx(anns, aid) {
                    Some(i) => i,
                    None => return Err(vec![unknown_id("annotation", aid, &ids_of(anns), path)]),
                };
                merge_patch(&mut anns[ai], v);
                let reset = v.get("text").is_some() || v.get("cite").is_some();
                downgrade_cites(&mut anns[ai], actor, reset, &format!("views/{}/annotations/{}", vid, aid), &mut diags);
                touch(&mut changed, aid);
            }
            ("remove", ["views", vid, "annotations", aid]) => {
                let views = d.get_mut("views").and_then(|c| c.as_array_mut()).ok_or_else(|| vec![unknown_id("view", vid, &[], path)])?;
                let vi = match find_idx(views, vid) {
                    Some(i) => i,
                    None => return Err(vec![unknown_id("view", vid, &ids_of(views), path)]),
                };
                let anns = views[vi].get_mut("annotations").and_then(|a| a.as_array_mut()).ok_or_else(|| vec![unknown_id("annotation", aid, &[], path)])?;
                match find_idx(anns, aid) {
                    Some(i) => {
                        anns.remove(i);
                    }
                    None => return Err(vec![unknown_id("annotation", aid, &ids_of(anns), path)]),
                }
                touch(&mut changed, aid);
            }
            (o, _) if !["add", "update", "remove", "set"].contains(&o) => {
                return Err(vec![perr(&at, format!("unknown op \"{}\". Allowed: add, update, remove, set", o), None)]);
            }
            _ => {
                return Err(vec![perr(&at, format!("op \"{}\" is not valid for path \"{}\"; {}", opn, path, PATH_HELP), None)]);
            }
        }
    }
    Ok(Applied { doc: d, changed, diags })
}

pub fn apply_result_json(ok: bool, doc: &Value, diags: &[Diag], summary: &str, changed: &[String]) -> Value {
    let doc_v = doc.clone();
    obj(vec![
        ("ok", Value::Bool(ok)),
        ("doc", doc_v),
        ("diagnostics", crate::diag::diags_json(diags)),
        ("summary", Value::String(summary.to_string())),
        ("changed", Value::Array(changed.iter().map(|c| Value::String(c.clone())).collect())),
    ])
}

pub fn has_errors(d: &[Diag]) -> bool {
    d.iter().any(|x| x.level == Level::Error)
}

#[allow(dead_code)]
fn _unused() {
    let _ = json::compact;
}
