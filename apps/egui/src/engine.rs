//! Thin facade over the Kerf engine API (SPEC §13). Everything crosses as JSON values, exactly
//! like the CLI / wasm ABI, so switching between the in-process `kerf-core` and the fixture
//! stub is a change in one function (`call_raw`).

use serde_json::{Value, json};

pub const DEFAULT_STYLE: &str = include_str!("../../../spec/styles/kerf-standard.kerfstyle.json");

pub const SAMPLES: [(&str, &str); 3] = [
    ("TRUSS-BEARING-CMU", include_str!("../../../spec/details/truss-bearing-cmu.kerf.json")),
    ("FLUSH-BEAM-STRAP", include_str!("../../../spec/details/flush-beam-strap.kerf.json")),
    ("MONOPOUR-SLAB-DOOR-RECESS", include_str!("../../../spec/details/monopour-slab-door-recess.kerf.json")),
];

pub fn default_style() -> Value {
    serde_json::from_str(DEFAULT_STYLE).expect("embedded style")
}

#[derive(Clone, Debug)]
pub struct ApplyOut {
    pub ok: bool,
    pub doc: Option<Value>,
    pub diagnostics: Vec<Value>,
    pub summary: String,
    pub changed: Vec<String>,
    /// raw response (for the tool_result text)
    pub raw: Value,
}

pub fn engine_name() -> &'static str {
    "kerf-core"
}

/// Raw engine call: function name + JSON input -> JSON output. Functions the in-process engine
/// does not implement yet fall back to the fixture stub (see `stub`).
fn call_raw(f: &str, input: &Value) -> Result<Value, String> {
    let text = serde_json::to_string(input).map_err(|e| e.to_string())?;
    match kerf_core::api::call(f, &text) {
        Ok(out) => {
            let bytes = out.bytes();
            serde_json::from_slice(&bytes).map_err(|e| format!("engine returned non-JSON for {f}: {e}"))
        }
        Err(e) if e.starts_with("unknown function") => stub::call(f, input),
        Err(e) => Err(e),
    }
}

pub fn call(f: &str, input: Value) -> Result<Value, String> {
    call_raw(f, &input)
}

pub fn apply(doc: &Value, style: &Value, ops: &Value, actor: &str) -> Result<ApplyOut, String> {
    let out = call_raw("apply", &json!({"doc": doc, "style": style, "ops": ops, "actor": actor}))?;
    Ok(ApplyOut {
        ok: out.get("ok").and_then(Value::as_bool).unwrap_or(false),
        doc: out.get("doc").cloned().filter(|d| !d.is_null()),
        diagnostics: out.get("diagnostics").and_then(Value::as_array).cloned().unwrap_or_default(),
        summary: out.get("summary").and_then(Value::as_str).unwrap_or("").to_owned(),
        changed: out
            .get("changed")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        raw: out,
    })
}

pub fn check(doc: &Value, style: &Value) -> Result<Value, String> {
    call_raw("check", &json!({"doc": doc, "style": style}))
}

pub fn inspect(doc: &Value, style: &Value, query: &Value) -> Result<Value, String> {
    call_raw("inspect", &json!({"doc": doc, "style": style, "query": query}))
}

pub fn drawing_json(doc: &Value, style: &Value, view: &str) -> Result<String, String> {
    let v = call_raw("drawing", &json!({"doc": doc, "style": style, "view": view}))?;
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

pub fn mesh_json(doc: &Value, style: &Value) -> Result<String, String> {
    let v = call_raw("mesh", &json!({"doc": doc, "style": style}))?;
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

pub fn catalog_markdown() -> String {
    match call_raw("catalog", &json!({"format": "markdown"})) {
        Ok(Value::String(s)) => s,
        Ok(v) => v.to_string(),
        Err(e) => format!("(catalog unavailable: {e})"),
    }
}

pub fn export(doc: &Value, style: &Value, view: &str, format: &str, sheet: bool) -> Result<Vec<u8>, String> {
    let input = json!({"doc": doc, "style": style, "view": view, "format": format, "sheet": sheet});
    kerf_core::api::call("export", &input.to_string()).map(|o| o.bytes())
}

/// JSON-level stand-in used until `kerf-core` lands: real op semantics on the document tree,
/// fixture drawing/mesh for every request.
mod stub {
    use super::*;

    const DRAWING: &str = include_str!("../fixtures/demo_drawing.json");
    const MESH: &str = include_str!("../fixtures/demo_mesh.json");

    pub fn call(f: &str, input: &Value) -> Result<Value, String> {
        match f {
            "version" => Ok(json!({"engine": "kerf-fixture-stub", "version": "0.0.0", "spec": "0.1"})),
            "drawing" => serde_json::from_str(DRAWING).map_err(|e| e.to_string()),
            "mesh" => serde_json::from_str(MESH).map_err(|e| e.to_string()),
            "catalog" => Ok(Value::String("(fixture stub: no catalog)".into())),
            "check" => Ok(json!({"diagnostics": [], "summary": summary(&input["doc"])})),
            "inspect" => Ok(json!({"text": summary(&input["doc"])})),
            "apply" => Ok(apply(input)),
            other => Err(format!("fixture stub: unknown fn {other}")),
        }
    }

    pub fn export(_doc: &Value, _style: &Value, _view: &str, _format: &str, _sheet: bool) -> Result<Vec<u8>, String> {
        Err("export needs the kerf-core engine (fixture stub)".into())
    }

    fn summary(doc: &Value) -> String {
        let comps = doc["components"].as_array().map(|a| a.len()).unwrap_or(0);
        let views = doc["views"].as_array().map(|a| a.len()).unwrap_or(0);
        let mut s = format!("DOC {}  {} components  {} views  0 errors 0 warnings\n", doc["id"].as_str().unwrap_or("?"), comps, views);
        for c in doc["components"].as_array().into_iter().flatten() {
            s += &format!(" {:<14}{}\n", c["id"].as_str().unwrap_or("?"), c["type"].as_str().unwrap_or("?"));
        }
        s
    }

    fn merge(target: &mut Value, patch: &Value) {
        if let Value::Object(p) = patch {
            if !target.is_object() {
                *target = json!({});
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        } else {
            *target = patch.clone();
        }
    }

    fn find_mut<'a>(arr: &'a mut Value, id: &str) -> Option<&'a mut Value> {
        arr.as_array_mut()?.iter_mut().find(|v| v["id"] == id)
    }

    fn apply_one(doc: &mut Value, op: &Value) -> Result<String, String> {
        let kind = op["op"].as_str().unwrap_or("");
        let path = op["path"].as_str().unwrap_or("");
        let seg: Vec<&str> = path.split('/').collect();
        let val = &op["value"];
        match (kind, seg.as_slice()) {
            ("set", ["doc"]) => {
                *doc = val.clone();
                Ok("doc".into())
            }
            ("add", ["components"]) => {
                let arr = doc["components"].as_array_mut().ok_or("doc has no components array")?;
                let id = val["id"].as_str().ok_or("component needs id")?.to_owned();
                if arr.iter().any(|c| c["id"] == id) {
                    return Err(format!("E_DUP_ID: component '{id}' already exists"));
                }
                match op["before"].as_str().and_then(|b| arr.iter().position(|c| c["id"] == b)) {
                    Some(i) => arr.insert(i, val.clone()),
                    None => arr.push(val.clone()),
                }
                Ok(id)
            }
            ("add", ["views"]) => {
                doc["views"].as_array_mut().ok_or("doc has no views array")?.push(val.clone());
                Ok(val["id"].as_str().unwrap_or("view").to_owned())
            }
            ("add", ["views", v, "annotations"]) => {
                let view = find_mut(&mut doc["views"], v).ok_or(format!("E_REF_UNKNOWN: view '{v}'"))?;
                if view["annotations"].is_null() {
                    view["annotations"] = json!([]);
                }
                view["annotations"].as_array_mut().unwrap().push(val.clone());
                Ok(val["id"].as_str().unwrap_or("annotation").to_owned())
            }
            ("update", ["components", id]) => {
                let c = find_mut(&mut doc["components"], id).ok_or(format!("E_REF_UNKNOWN: component '{id}'"))?;
                merge(c, val);
                Ok((*id).to_owned())
            }
            ("update", ["views", id]) => {
                let c = find_mut(&mut doc["views"], id).ok_or(format!("E_REF_UNKNOWN: view '{id}'"))?;
                merge(c, val);
                Ok((*id).to_owned())
            }
            ("update", ["views", v, "annotations", id]) => {
                let view = find_mut(&mut doc["views"], v).ok_or(format!("E_REF_UNKNOWN: view '{v}'"))?;
                let a = find_mut(&mut view["annotations"], id).ok_or(format!("E_REF_UNKNOWN: annotation '{id}'"))?;
                merge(a, val);
                Ok((*id).to_owned())
            }
            ("update", ["meta"]) => {
                merge(&mut doc["meta"], val);
                Ok("meta".into())
            }
            ("remove", ["components", id]) => remove(&mut doc["components"], id),
            ("remove", ["views", id]) => remove(&mut doc["views"], id),
            ("remove", ["views", v, "annotations", id]) => {
                let view = find_mut(&mut doc["views"], v).ok_or(format!("E_REF_UNKNOWN: view '{v}'"))?;
                remove(&mut view["annotations"], id)
            }
            _ => Err(format!("E_PARAM: unsupported op {kind} {path}")),
        }
    }

    fn remove(arr: &mut Value, id: &str) -> Result<String, String> {
        let a = arr.as_array_mut().ok_or("not an array")?;
        let i = a.iter().position(|c| c["id"] == id).ok_or(format!("E_REF_UNKNOWN: '{id}'"))?;
        a.remove(i);
        Ok(id.to_owned())
    }

    fn apply(input: &Value) -> Value {
        let mut doc = input["doc"].clone();
        let actor = input["actor"].as_str().unwrap_or("llm");
        let mut changed = vec![];
        let mut diags = vec![];
        for op in input["ops"].as_array().into_iter().flatten() {
            match apply_one(&mut doc, op) {
                Ok(id) => changed.push(id),
                Err(e) => {
                    return json!({"ok": false, "doc": null, "diagnostics": [{"level": "error", "code": "E_PARAM", "message": e}], "summary": "", "changed": []});
                }
            }
        }
        if actor != "designer" {
            // an LLM can never verify a citation
            for v in doc["views"].as_array_mut().into_iter().flatten() {
                for a in v["annotations"].as_array_mut().into_iter().flatten() {
                    for c in a["cite"].as_array_mut().into_iter().flatten() {
                        if c["status"] == "verified" {
                            c["status"] = json!("suggested");
                            diags.push(json!({"level": "info", "code": "I_CITE_DOWNGRADED", "message": "citation verification is designer-only"}));
                        }
                    }
                }
            }
        }
        json!({"ok": true, "doc": doc, "diagnostics": diags, "summary": summary(&doc), "changed": changed})
    }
}
