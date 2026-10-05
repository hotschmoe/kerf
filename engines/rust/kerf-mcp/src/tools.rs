//! The Kerf tool surface. Names and semantics mirror spec/llm/tools.json (kerf_apply,
//! kerf_inspect, kerf_render) plus document management, export and catalog. The descriptions
//! below ARE the user interface for the model: keep them precise.

use crate::raster::{base64, svg_to_png};
use crate::store::{Workspace, doc_id_of};
use crate::text::diag_line;
use kerf_core::api;
use serde_json::{Value, json};

pub const RENDER_WIDTH_PX: u32 = 1400;

pub struct Outcome {
    pub content: Vec<Value>,
    pub is_error: bool,
}

fn text(s: impl Into<String>) -> Outcome {
    Outcome { content: vec![json!({"type": "text", "text": s.into()})], is_error: false }
}

fn err(s: impl Into<String>) -> Outcome {
    Outcome { content: vec![json!({"type": "text", "text": s.into()})], is_error: true }
}

pub fn definitions() -> Value {
    let doc_prop = json!({"type": "string", "description": "Optional document id (or path ending in .kerf.json) to act on instead of the active document."});
    json!([
        {
            "name": "kerf_new",
            "title": "New Kerf document",
            "description": "Create an empty Kerf document <id>.kerf.json in the workspace and make it the active document (all other tools act on the active document). Fails if the id already exists (use kerf_open). After this, send ONE kerf_apply with {\"op\":\"set\",\"path\":\"doc\",\"value\":{...whole document...}} containing the components, a section view and notes; check kerf_catalog (or resource kerf://catalog) for component params first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Document id and file name stem, e.g. \"truss-bearing-cmu\" (letters, digits, - _ .)."},
                    "title": {"type": "string", "description": "Detail title in drawing grammar (UPPERCASE), e.g. \"TRUSS BEARING AT CMU WALL\"."},
                    "meta": {"type": "object", "description": "Optional document meta, e.g. {\"jurisdiction\":{\"code\":\"IRC\",\"edition\":2021},\"discipline\":\"structural\",\"sheet\":\"S-502\"}."}
                },
                "required": ["id", "title"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false}
        },
        {
            "name": "kerf_open",
            "title": "Open Kerf document",
            "description": "Make an existing document the active one and return its summary (components with resolved extents, views, diagnostics). `path` is a document id from kerf_list (e.g. \"flush-beam-strap\") or a path ending in .kerf.json (relative paths resolve against the workspace).",
            "inputSchema": {
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Document id or path to a *.kerf.json file."}},
                "required": ["path"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        },
        {
            "name": "kerf_list",
            "title": "List Kerf documents",
            "description": "List the documents in the workspace directory (id, title, component and view counts) and which one is active.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        },
        {
            "name": "kerf_apply",
            "title": "Apply edit ops",
            "description": "Apply a batch of edit ops to the active Kerf document, atomically, and save it. Returns ok or FAILED, the updated summary (every component with resolved x/y extents in feet-inches) and diagnostics. If any op fails NOTHING changes (the file is untouched): read the errors and their Fix hints, correct, resend the whole batch. Fix every error and fix or justify every warning. For a brand-new detail send one {\"op\":\"set\",\"path\":\"doc\",\"value\":{...whole document...}}; afterwards prefer small ops so the designer sees clean diffs. Ops: add (path \"components\" | \"views\" | \"views/<id>/annotations\"; value = the item; optional \"before\":\"<id>\" to insert before an existing item), update (path \"components/<id>\" | \"views/<id>\" | \"views/<id>/annotations/<id>\" | \"meta\"; value = RFC 7396 JSON merge patch, null deletes a key), remove (path to one item; fails with E_REF_UNKNOWN naming dependents if still referenced), set (path \"doc\"). Position by anchors, e.g. \"at\":{\"anchor\":\"bottom_left\",\"to\":\"bond_beam@top_left\"}. Any citation you write is stored with status \"suggested\"; only the designer can verify. After a significant change call kerf_render and look at the picture.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "ops": {
                        "type": "array",
                        "description": "Edit ops, applied in order, all-or-nothing.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "op": {"type": "string", "enum": ["add", "update", "remove", "set"]},
                                "path": {"type": "string", "description": "e.g. \"components\", \"components/sill_plate\", \"views/A/annotations\", \"meta\", \"doc\"."},
                                "value": {"type": "object", "description": "Item (add), merge patch (update) or whole document (set). Omit for remove."},
                                "before": {"type": "string", "description": "add only: id of the existing item to insert before."}
                            },
                            "required": ["op", "path"]
                        }
                    },
                    "why": {"type": "string", "description": "One short line for the designer's change log, e.g. \"Add HETA20 embedded truss anchor\"."},
                    "doc": doc_prop
                },
                "required": ["ops", "why"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false}
        },
        {
            "name": "kerf_inspect",
            "title": "Inspect document",
            "description": "Read the active document without changing it. q: 'summary' (component table + all diagnostics, including per-view note/fit warnings), 'component' (resolved params, parts, every anchor with coordinates in inches and ft-in, z range; needs id), 'anchors' (anchors only; needs id), 'at' (components under point [x, y] in inches, in a view), 'catalog' (one component type's params and anchors; needs type), 'doc' (the full canonical JSON). Use 'component'/'anchors' to find the exact anchor names before positioning something relative to it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": {"type": "string", "enum": ["summary", "component", "anchors", "at", "catalog", "doc"]},
                    "id": {"type": "string", "description": "Component id (q = component | anchors)."},
                    "type": {"type": "string", "description": "Component type, e.g. \"truss\" (q = catalog)."},
                    "point": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[x, y] in inches (q = at)."},
                    "view": {"type": "string", "description": "View id (q = at); defaults to the first view."},
                    "doc": doc_prop
                },
                "required": ["q"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        },
        {
            "name": "kerf_render",
            "title": "Render a view",
            "description": "Render a view exactly as it will export and return it as a PNG image (about 1400 px wide, white background, true pen weights) plus a one-line text with scale, note count and diagnostic counts. Use it to check your work: overlaps, gaps, leader clutter, notes pointing at the wrong thing, proportions vs the designer's reference. Call after every significant change and before telling the designer you are done. mode 'view' = the cropped drawing (default); 'sheet' = full page with frame and title block.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view": {"type": "string", "description": "View id, e.g. \"A\". Defaults to the first view."},
                    "mode": {"type": "string", "enum": ["view", "sheet"], "description": "view = cropped drawing (default); sheet = full page with title block."},
                    "doc": doc_prop
                },
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        },
        {
            "name": "kerf_export",
            "title": "Export a view",
            "description": "Export a view to a file and return its path and size. format: svg, dxf (layered CAD, units inches) or pdf (always a full sheet with title block, vector). sheet (svg/dxf): true adds the frame and title block. The file is written inside the workspace: default exports/<id>-<view>.<ext>; `path` may be relative to the workspace or absolute inside it. Overwrites an existing file at that path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view": {"type": "string", "description": "View id, e.g. \"A\". Defaults to the first view."},
                    "format": {"type": "string", "enum": ["svg", "dxf", "pdf"]},
                    "sheet": {"type": "boolean", "description": "svg/dxf: include frame + title block (default false). pdf is always a sheet."},
                    "path": {"type": "string", "description": "Output file, relative to the workspace (default exports/<id>-<view>.<format>)."},
                    "doc": doc_prop
                },
                "required": ["format"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        },
        {
            "name": "kerf_catalog",
            "title": "Component catalog",
            "description": "The component catalog. Without `type`: markdown listing every component type, its params (with defaults and allowed values) and its named anchors. With `type` (e.g. \"cmu_wall\"): that type's entry as JSON. Read this before writing a document; do not guess parameter names.",
            "inputSchema": {
                "type": "object",
                "properties": {"type": {"type": "string", "description": "Component type: lumber, panel, cmu_wall, concrete, rebar, anchor_bolt, connector, truss, membrane, fill, insulation, solid."}},
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        }
    ])
}

fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn style_of(ws: &Workspace) -> Value {
    ws.style.clone().unwrap_or(Value::Null)
}

fn engine(f: &str, input: &Value) -> Result<api::Output, String> {
    api::call(f, &input.to_string())
}

fn engine_json(f: &str, input: &Value) -> Result<Value, String> {
    let out = engine(f, input)?.bytes();
    serde_json::from_slice(&out).map_err(|e| format!("engine returned invalid JSON: {}", e))
}

/// Run one tool. `None` = unknown tool name.
pub fn call(ws: &mut Workspace, name: &str, args: &Value) -> Option<Outcome> {
    let r = match name {
        "kerf_new" => new_doc(ws, args),
        "kerf_open" => open_doc(ws, args),
        "kerf_list" => list_docs(ws),
        "kerf_apply" => apply(ws, args),
        "kerf_inspect" => inspect(ws, args),
        "kerf_render" => render(ws, args),
        "kerf_export" => export(ws, args),
        "kerf_catalog" => catalog(args),
        _ => return None,
    };
    Some(r.unwrap_or_else(err))
}

type R = Result<Outcome, String>;

fn views_of(doc: &Value) -> Vec<(String, String, String)> {
    doc.get("views")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|v| {
                    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    (s("id"), s("kind"), s("scale"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn views_line(doc: &Value) -> String {
    let v = views_of(doc);
    if v.is_empty() {
        return "Views: (none yet: add one with kerf_apply {\"op\":\"add\",\"path\":\"views\",\"value\":{...}})".into();
    }
    format!("Views: {}", v.iter().map(|(id, kind, scale)| format!("{} ({}{}{})", id, if kind.is_empty() { "section" } else { kind }, if scale.is_empty() { "" } else { ", " }, scale)).collect::<Vec<_>>().join("; "))
}

fn pick_view(doc: &Value, requested: Option<&str>) -> Result<String, String> {
    let views = views_of(doc);
    match requested {
        Some(v) => {
            if views.iter().any(|(id, _, _)| id == v) {
                Ok(v.to_string())
            } else if views.is_empty() {
                Err(format!("view {:?} not found: the document has no views. Add one with kerf_apply {{\"op\":\"add\",\"path\":\"views\",\"value\":{{...}}}}.", v))
            } else {
                Err(format!("view {:?} not found. Views: {}.", v, views.iter().map(|(id, _, _)| id.as_str()).collect::<Vec<_>>().join(", ")))
            }
        }
        None => views.first().map(|(id, _, _)| id.clone()).ok_or_else(|| "the document has no views. Add one with kerf_apply {\"op\":\"add\",\"path\":\"views\",\"value\":{...}}.".to_string()),
    }
}

fn new_doc(ws: &mut Workspace, a: &Value) -> R {
    let id = arg_str(a, "id").ok_or("kerf_new needs \"id\" (file name stem, e.g. \"truss-bearing-cmu\") and \"title\"")?;
    let title = arg_str(a, "title").ok_or("kerf_new needs \"title\", e.g. \"TRUSS BEARING AT CMU WALL\"")?;
    let path = ws.path_for_id(id)?;
    if path.exists() {
        return Err(format!("{} already exists. Use kerf_open {{\"path\":\"{}\"}} to continue editing it, or choose another id.", path.display(), id));
    }
    let mut doc = json!({"kerf": "0.1", "id": id, "title": title});
    if let Some(m) = a.get("meta").filter(|m| m.is_object()) {
        doc["meta"] = m.clone();
    }
    doc["components"] = json!([]);
    doc["views"] = json!([]);
    let fm = engine_json("fmt", &json!({"doc": doc}))?;
    let t = fm["text"].as_str().ok_or("engine fmt returned no text")?;
    Workspace::save_text(&path, t)?;
    let _ = Workspace::append_log(&path, "new document", &json!([]), &json!([]));
    ws.set_active(path.clone());
    Ok(text(format!(
        "Created {} and made it the active document.\nNext: read kerf_catalog, then send ONE kerf_apply with {{\"op\":\"set\",\"path\":\"doc\",\"value\":{{...whole document: components, a section view with crop + scale, notes...}}}}. Keep \"id\":\"{}\".",
        path.display(),
        id
    )))
}

fn open_doc(ws: &mut Workspace, a: &Value) -> R {
    let spec = arg_str(a, "path").ok_or("kerf_open needs \"path\": a document id from kerf_list or a path ending in .kerf.json")?;
    let path = ws.resolve_spec(spec)?;
    let doc = Workspace::load(&path)?;
    let chk = engine_json("check", &json!({"doc": doc, "style": style_of(ws)}))?;
    ws.set_active(path.clone());
    Ok(text(format!("Opened {} (now the active document).\n{}\n{}", path.display(), chk["summary"].as_str().unwrap_or("").trim_end(), views_line(&doc))))
}

fn list_docs(ws: &Workspace) -> R {
    let files = ws.list();
    if files.is_empty() {
        return Ok(text(format!("No documents in {}. Create one with kerf_new {{\"id\":\"my-detail\",\"title\":\"...\"}}.", ws.dir().display())));
    }
    let mut out = format!("Workspace {}\n", ws.dir().display());
    for p in &files {
        let id = doc_id_of(p);
        let mark = if ws.active() == Some(p.as_path()) { "  <- active" } else { "" };
        match Workspace::load(p) {
            Ok(d) => {
                let n = d.get("components").and_then(|c| c.as_array()).map_or(0, |c| c.len());
                let v = d.get("views").and_then(|c| c.as_array()).map_or(0, |c| c.len());
                out.push_str(&format!("  {}  {:?}  {} components, {} views{}\n", id, d.get("title").and_then(|t| t.as_str()).unwrap_or(""), n, v, mark));
            }
            Err(e) => out.push_str(&format!("  {}  (unreadable: {}){}\n", id, e, mark)),
        }
    }
    if ws.active().is_none() {
        out.push_str("No active document: kerf_open {\"path\":\"<id>\"} (or pass \"doc\":\"<id>\" to a tool).\n");
    }
    Ok(text(out.trim_end().to_string()))
}

fn apply(ws: &mut Workspace, a: &Value) -> R {
    let path = ws.target(arg_str(a, "doc"))?;
    let mut ops = a.get("ops").cloned().ok_or("kerf_apply needs \"ops\": an array of {op, path, value}")?;
    if let Some(s) = ops.as_str() {
        ops = kerf_core::json::parse(s).map_err(|e| format!("\"ops\" must be a JSON array of ops, not a string ({})", e))?;
    }
    if !ops.is_array() {
        return Err("\"ops\" must be an array, e.g. [{\"op\":\"update\",\"path\":\"components/sill\",\"value\":{\"treated\":true}}]".into());
    }
    if ops.as_array().is_some_and(|o| o.is_empty()) {
        return Err("\"ops\" is empty: nothing to apply".into());
    }
    let why = arg_str(a, "why").unwrap_or("edit");
    let doc = Workspace::load(&path)?;
    // actor is fixed to "llm": the engine downgrades any `verified` citation to `suggested`.
    let r = engine_json("apply", &json!({"doc": doc, "style": style_of(ws), "ops": ops, "actor": "llm"}))?;
    let summary = r["summary"].as_str().unwrap_or("").trim_end();
    let diags: Vec<Value> = r["diagnostics"].as_array().cloned().unwrap_or_default();
    if r["ok"].as_bool() != Some(true) {
        let mut out = String::from("FAILED: nothing was changed (the document on disk is untouched). Fix these and resend the whole batch:\n");
        for d in diags.iter().filter(|d| d["level"] == "error") {
            out.push_str(&diag_line(d));
            out.push('\n');
        }
        for d in diags.iter().filter(|d| d["level"] != "error") {
            out.push_str(&diag_line(d));
            out.push('\n');
        }
        let now = engine_json("check", &json!({"doc": doc, "style": style_of(ws)}))?;
        out.push_str(&format!("Document still: {}", now["summary"].as_str().unwrap_or("").lines().next().unwrap_or("")));
        return Ok(err(out));
    }
    let canon = &r["doc"];
    Workspace::save_text(&path, &kerf_core::json::pretty(canon))?;
    let changed = r["changed"].clone();
    let log_note = match Workspace::append_log(&path, why, &ops, &changed) {
        Ok(()) => String::new(),
        Err(e) => format!("\nWARNING: change applied and saved, but the op log could not be written: {}", e),
    };
    let ch: Vec<&str> = changed.as_array().map(|c| c.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let mut out = format!(
        "ok: applied {} op(s){}; saved {}\n{}",
        ops.as_array().map_or(0, |o| o.len()),
        if ch.is_empty() { String::new() } else { format!(", changed: {}", ch.join(", ")) },
        path.display(),
        summary
    );
    let file_id = doc_id_of(&path);
    if canon["id"].as_str().is_some_and(|i| i != file_id) {
        out.push_str(&format!("\nNOTE: the document's \"id\" is now {:?} but the file is {}.kerf.json; keep \"id\" equal to the file name.", canon["id"].as_str().unwrap_or(""), file_id));
    }
    if views_of(canon).is_empty() {
        out.push_str("\nNOTE: the document has no views yet; add a section view (kerf_apply add path \"views\") before rendering.");
    }
    out.push_str(&log_note);
    ws.set_active(path);
    Ok(text(out))
}

fn inspect(ws: &mut Workspace, a: &Value) -> R {
    let q = arg_str(a, "q").unwrap_or("summary");
    if q == "catalog" {
        // does not need a document
        return catalog(&json!({"type": a.get("type").cloned().unwrap_or(Value::Null)}));
    }
    let path = ws.target(arg_str(a, "doc"))?;
    let doc = Workspace::load(&path)?;
    match q {
        "summary" => {
            let chk = engine_json("check", &json!({"doc": doc, "style": style_of(ws)}))?;
            Ok(text(format!("{}\n{}", chk["summary"].as_str().unwrap_or("").trim_end(), views_line(&doc))))
        }
        "doc" => {
            let fm = engine_json("fmt", &json!({"doc": doc}))?;
            Ok(text(fm["text"].as_str().unwrap_or("").to_string()))
        }
        "component" | "anchors" | "at" => {
            let mut query = json!({"q": q});
            for k in ["id", "point"] {
                if let Some(v) = a.get(k) {
                    query[k] = v.clone();
                }
            }
            if q == "at" {
                query["view"] = json!(pick_view(&doc, arg_str(a, "view"))?);
            }
            let r = engine_json("inspect", &json!({"doc": doc, "style": style_of(ws), "query": query}))?;
            Ok(text(kerf_core::json::compact(&r)))
        }
        other => Err(format!("unknown q {:?}: use summary, component, anchors, at, catalog or doc", other)),
    }
}

fn catalog(a: &Value) -> R {
    match arg_str(a, "type") {
        None => Ok(text(kerf_core::catalog::markdown())),
        Some(t) => {
            let cat = kerf_core::catalog::json();
            let types: Vec<&str> = cat["types"].as_array().map(|x| x.iter().filter_map(|e| e["type"].as_str()).collect()).unwrap_or_default();
            match cat["types"].as_array().and_then(|x| x.iter().find(|e| e["type"] == t)) {
                Some(e) => Ok(text(kerf_core::json::pretty(e))),
                None => Err(format!("unknown component type {:?}. Types: {}.", t, types.join(", "))),
            }
        }
    }
}

struct Rendered {
    svg: String,
    view: String,
    sheet: bool,
}

fn export_svg(ws: &Workspace, a: &Value, sheet: bool) -> Result<(Rendered, Value, Value), String> {
    let path = ws.target(arg_str(a, "doc"))?;
    let doc = Workspace::load(&path)?;
    let view = pick_view(&doc, arg_str(a, "view"))?;
    let style = style_of(ws);
    let svg = engine("export", &json!({"doc": doc, "style": style, "view": view, "format": "svg", "sheet": sheet}))?.bytes();
    let svg = String::from_utf8(svg).map_err(|e| e.to_string())?;
    let drawing = engine_json("drawing", &json!({"doc": doc, "style": style, "view": view}))?;
    Ok((Rendered { svg, view, sheet }, doc, drawing))
}

fn scale_label(doc: &Value, view: &str, drawing: &Value) -> String {
    let s = doc["views"].as_array().and_then(|v| v.iter().find(|x| x["id"] == view)).and_then(|v| v["scale"].as_str()).unwrap_or("");
    if s.trim().eq_ignore_ascii_case("nts") {
        return "NTS".into();
    }
    kerf_core::num::scale_text(s, drawing["scale"].as_f64())
}

fn render(ws: &mut Workspace, a: &Value) -> R {
    let sheet = match arg_str(a, "mode").unwrap_or("view") {
        "view" => false,
        "sheet" => true,
        other => return Err(format!("unknown mode {:?}: use \"view\" or \"sheet\"", other)),
    };
    let (r, doc, drawing) = export_svg(ws, a, sheet)?;
    let png = svg_to_png(&r.svg, RENDER_WIDTH_PX)?;
    let chk = engine_json("check", &json!({"doc": doc, "style": style_of(ws)}))?;
    let count = |lvl: &str| chk["diagnostics"].as_array().map_or(0, |d| d.iter().filter(|x| x["level"] == lvl).count());
    let notes = doc["views"].as_array().and_then(|v| v.iter().find(|x| x["id"] == r.view)).and_then(|v| v["annotations"].as_array()).map_or(0, |an| an.iter().filter(|x| x["type"] == "note").count());
    let kind = drawing["kind"].as_str().unwrap_or("section");
    let mut line = format!(
        "{} {} ({}) rendered at {}; {} note(s); {} error(s), {} warning(s)",
        if r.sheet { "sheet" } else { "view" },
        r.view,
        kind,
        scale_label(&doc, &r.view, &drawing),
        notes,
        count("error"),
        count("warning")
    );
    for d in drawing["diagnostics"].as_array().into_iter().flatten() {
        line.push('\n');
        line.push_str(&diag_line(d));
    }
    Ok(Outcome {
        content: vec![json!({"type": "image", "data": base64(&png.bytes), "mimeType": "image/png"}), json!({"type": "text", "text": line})],
        is_error: false,
    })
}

fn export(ws: &mut Workspace, a: &Value) -> R {
    let format = arg_str(a, "format").ok_or("kerf_export needs \"format\": svg, dxf or pdf")?;
    if !matches!(format, "svg" | "dxf" | "pdf") {
        return Err(format!("unsupported export format {:?}: use svg, dxf or pdf", format));
    }
    let docpath = ws.target(arg_str(a, "doc"))?;
    let doc = Workspace::load(&docpath)?;
    let view = pick_view(&doc, arg_str(a, "view"))?;
    let sheet = format == "pdf" || a.get("sheet").and_then(|s| s.as_bool()).unwrap_or(false);
    let bytes = engine("export", &json!({"doc": doc, "style": style_of(ws), "view": view, "format": format, "sheet": sheet}))?.bytes();
    let default_name = format!("{}-{}{}.{}", doc_id_of(&docpath), view, if sheet && format != "pdf" { "-sheet" } else { "" }, format);
    let out = ws.export_path(arg_str(a, "path"), &default_name)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
    }
    std::fs::write(&out, &bytes).map_err(|e| format!("cannot write {}: {}", out.display(), e))?;
    Ok(text(format!("Wrote {} ({} bytes; {} view {}{})", out.display(), bytes.len(), format, view, if sheet { ", sheet" } else { "" })))
}
