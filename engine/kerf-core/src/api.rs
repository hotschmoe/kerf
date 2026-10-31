//! The engine API (SPEC 13): `call(fn, input_json) -> output`, identical for CLI, wasm and in-process callers.

use crate::canon;
use crate::catalog;
use crate::drawing;
use crate::drawview;
use crate::dxf;
use crate::font;
use crate::json::{self, Value};
use crate::limits;
use crate::lint;
use crate::load;
use crate::mesh;
use crate::model::{self, Diag, Diags, Level};
use crate::ops::{self, Actor};
use crate::pdf;
use crate::raster;
use crate::references;
use crate::review;
use crate::schema;
use crate::schema_fields;
use crate::sheet;
use crate::style::{self, Style, StyleError};
use crate::svg;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ENGINE_NAME: &str = "kerf";

/// `ok` false means `bytes` is `{"error":{code,message}}`.
pub struct Output {
    pub ok: bool,
    pub bytes: Vec<u8>,
}

fn err_json(code: &str, message: &str) -> Vec<u8> {
    let mut out = Vec::from(&b"{\"error\":{\"code\":"[..]);
    json::write_string(&mut out, code);
    out.extend_from_slice(b",\"message\":");
    json::write_string(&mut out, message);
    out.extend_from_slice(b"}}\n");
    out
}

fn fail(code: &str, message: &str) -> Output {
    Output { ok: false, bytes: err_json(code, message) }
}

fn done(bytes: Vec<u8>) -> Output {
    Output { ok: true, bytes }
}

pub const FN_NAMES: [&str; 11] =
    ["version", "help", "catalog", "schema", "fmt", "check", "apply", "inspect", "drawing", "mesh", "export"];

const HELP_ROWS: [[&str; 3]; 11] = [
    ["version", "{}", "{engine, version, spec}"],
    ["help", "{}", "this list"],
    ["catalog", "{format?: \"json\"|\"markdown\"}", "the component catalog (json default)"],
    [
        "schema",
        "{topic?: \"view\"|\"note\"|\"dim\"|\"label\"|\"cite\"|\"ops\"|\"doc\"|<component type>}",
        "{topic, text}: field reference text; no topic lists the topics",
    ],
    ["fmt", "{doc}", "{doc, text}: canonical key order and indentation"],
    [
        "check",
        "{doc, style?, strict?: bool, reference_files?: {path: \"missing\"|\"outside\"}}",
        "{diagnostics: [{level, code, id?, path?, message, fix?}], acknowledged: [{code, id, reason, path?, message, fix?}], summary}: strict keeps acknowledged warnings as warnings",
    ],
    [
        "apply",
        "{doc, ops: [op...], style?, actor?: \"llm\"|\"designer\"}",
        "{ok, doc, diagnostics, acknowledged, summary, changed}: ok=false leaves doc unchanged (see diagnostics); `kerf schema ops` for op shapes",
    ],
    [
        "inspect",
        "{doc, style?, query: {q: \"summary\"|\"component\"|\"anchors\"|\"at\"|\"catalog\"|\"doc\", id?, view?, point?: [x,y], type?}}",
        "query-specific JSON (anchors with coordinates, parts, what is visible at a point)",
    ],
    [
        "drawing",
        "{doc, view, style?}",
        "Drawing IR JSON (SPEC 10) of one view; `acknowledged` lists the suppressed warnings of its components when the doc has `acknowledge`",
    ],
    ["mesh", "{doc, style?, include_fills?: bool}", "mesh JSON for 3D viewers"],
    [
        "export",
        "{doc, view, format: \"svg\"|\"png\"|\"pdf\"|\"dxf\", sheet?: bool, review?: bool, px?: number, style?}",
        "the file bytes (svg/dxf text, png/pdf binary); review marks acknowledged components and stamps N ACKNOWLEDGED WARNINGS",
    ],
];

fn help_fn() -> Output {
    let mut out = Vec::from(&b"{\"functions\":[\n"[..]);
    for (i, row) in HELP_ROWS.iter().enumerate() {
        out.extend_from_slice(b"{\"name\":");
        json::write_string(&mut out, row[0]);
        out.extend_from_slice(b",\"input\":");
        json::write_string(&mut out, row[1]);
        out.extend_from_slice(b",\"output\":");
        json::write_string(&mut out, row[2]);
        out.extend_from_slice(if i + 1 < HELP_ROWS.len() { b"},\n" } else { b"}\n" });
    }
    out.extend_from_slice(b"],\"note\":\"input is one JSON object on stdin; doc is the document object; a failure is {error: {code, message}} with exit 1\"}\n");
    done(out)
}

fn schema_fn(inp: &Value) -> Output {
    let topic = inp.get("topic").and_then(Value::str);
    let text = match topic {
        Some(tp) => {
            if let Some(t) = schema::render(tp) {
                t
            } else {
                let mut names: Vec<&str> = schema_fields::OBJECTS.iter().map(|o| o.name).collect();
                names.extend(catalog::ENTRIES.iter().map(super::catalog::Entry::name));
                let hint = model::nearest(tp, &names).map(|n| format!(" Did you mean \"{n}\"?")).unwrap_or_default();
                return fail(
                    "E_TOPIC",
                    &format!("unknown schema topic \"{tp}\".{hint} Topics: {}", schema::TOPICS_HINT),
                );
            }
        }
        None => schema::index(),
    };
    let mut out = Vec::from(&b"{\"topic\":"[..]);
    json::write_string(&mut out, topic.unwrap_or(""));
    out.extend_from_slice(b",\"text\":");
    json::write_string(&mut out, &text);
    out.extend_from_slice(b"}\n");
    done(out)
}

/// Runs one API function. `name` and `input` are raw bytes (the wasm host passes whatever it wrote).
pub fn call(name: &[u8], input: &[u8]) -> Output {
    dispatch(name, input)
}

pub fn dispatch(name: &[u8], input: &[u8]) -> Output {
    if input.len() > limits::MAX_JSON_BYTES {
        return fail(
            "E_LIMIT",
            &limits::message(
                "input too large (bytes)",
                input.len(),
                limits::MAX_JSON_BYTES,
                "Split the detail into several documents (one sheet per document) or drop unused components and views.",
            ),
        );
    }
    let trimmed = input.trim_ascii();
    let inp = if trimmed.is_empty() {
        Value::Object(Vec::new())
    } else {
        match json::parse(input) {
            Ok(v) => v,
            Err(e) => {
                return fail(
                    "E_JSON",
                    &format!("input is not valid JSON: {} (line {}, column {})", e.msg, e.line, e.col),
                );
            }
        }
    };
    if !matches!(inp, Value::Object(_)) {
        return fail("E_INPUT", "input must be a JSON object");
    }
    match name {
        b"version" => {
            done(format!("{{\"engine\":\"{ENGINE_NAME}\",\"version\":\"{VERSION}\",\"spec\":\"0.1\"}}\n").into_bytes())
        }
        b"help" => help_fn(),
        b"catalog" => catalog_fn(&inp),
        b"schema" => schema_fn(&inp),
        b"fmt" => fmt_fn(&inp),
        b"check" => check_fn(&inp),
        b"apply" => apply_fn(&inp),
        b"inspect" => inspect_fn(&inp),
        b"drawing" => drawing_fn(&inp),
        b"mesh" => mesh_fn(&inp),
        b"export" => export_fn(&inp),
        _ => fail(
            "E_FN",
            &format!(
                "unknown function '{}'. Functions: {} (`kerf call help` lists their input shapes)",
                String::from_utf8_lossy(name),
                FN_NAMES.join(", ")
            ),
        ),
    }
}

fn catalog_fn(inp: &Value) -> Output {
    let fmt_v = inp.get("format").map_or("json", |f| f.str().unwrap_or("json"));
    if fmt_v == "markdown" || fmt_v == "md" {
        return done(catalog::catalog_markdown().into_bytes());
    }
    if fmt_v != "json" {
        return fail("E_INPUT", "catalog format must be \"json\" or \"markdown\"");
    }
    let mut out = Vec::new();
    json::write_compact(&mut out, &catalog::catalog_json());
    out.push(b'\n');
    done(out)
}

fn get_doc(inp: &Value) -> Result<&Value, Output> {
    match inp.get("doc") {
        None => Err(fail("E_INPUT", "missing \"doc\": the Kerf document object")),
        Some(d @ Value::Object(_)) => Ok(d),
        Some(d) => {
            Err(fail("E_INPUT", &format!("\"doc\" must be a JSON object (the document), not {}", d.kind_name())))
        }
    }
}

fn get_style(inp: &Value) -> Result<Style, Output> {
    let user = inp.get("style").filter(|s| matches!(s, Value::Object(_)));
    style::load(user).map_err(|StyleError::BadStyle(why)| {
        if why.is_empty() {
            fail("E_STYLE", "the style is not a valid kerfstyle (needs pens and materials objects)")
        } else {
            fail("E_STYLE", &format!("the style has invalid values: {why}. Fix: remove those keys to use the defaults (shown above), or give values inside the stated ranges."))
        }
    })
}

macro_rules! doc_and_style {
    ($inp:expr) => {
        match (get_doc($inp), get_style($inp)) {
            (Ok(d), Ok(st)) => (d, st),
            (Err(e), _) | (_, Err(e)) => return e,
        }
    };
}

fn fmt_fn(inp: &Value) -> Output {
    let d = match get_doc(inp) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let text = canon::write(d);
    let mut out = Vec::from(&b"{\"doc\":"[..]);
    out.extend_from_slice(trim_newlines(&text));
    out.extend_from_slice(b",\"text\":");
    json::write_string(&mut out, &String::from_utf8_lossy(&text));
    out.extend_from_slice(b"}\n");
    done(out)
}

fn trim_newlines(b: &[u8]) -> &[u8] {
    let n = b.iter().rposition(|&c| c != b'\n').map_or(0, |i| i + 1);
    &b[..n]
}

fn build_view<'s>(inp: &Value, d: &Value, st: &'s Style) -> Result<drawing::Drawing<'s>, Output> {
    let Some(view_id) = inp.get("view").and_then(Value::str) else {
        return Err(fail("E_INPUT", "missing \"view\": the id of a view in the document"));
    };
    let diags = Diags::new();
    let vdoc = lint::with_dim_dirs_from_doc(d, st);
    let mut dr = drawview::build(&vdoc, st, view_id, &diags).ok_or_else(|| {
        let msg = diags.list().first().map_or_else(|| "view could not be built".to_string(), |x| x.message.clone());
        fail("E_VIEW", &msg)
    })?;
    let review = matches!(inp.get("review"), Some(Value::Bool(true)));
    if review || lint::has_acknowledge(d) {
        dr.acknowledged = Some(load::view_acked(d, st, view_id, &dr.diagnostics));
    }
    Ok(dr)
}

fn drawing_fn(inp: &Value) -> Output {
    let (d, st) = doc_and_style!(inp);
    match build_view(inp, d, &st) {
        Ok(dr) => done(drawing::to_json(&dr)),
        Err(e) => e,
    }
}

fn export_fn(inp: &Value) -> Output {
    let (d, st) = doc_and_style!(inp);
    let dr = match build_view(inp, d, &st) {
        Ok(dr) => dr,
        Err(e) => return e,
    };
    let format = inp.get("format").and_then(Value::str).unwrap_or("svg");
    let font = font::embedded();
    let want_sheet = matches!(inp.get("sheet"), Some(Value::Bool(true)));
    let review = matches!(inp.get("review"), Some(Value::Bool(true)));
    let maybe_sheet = |dr, want: bool| match (want, review) {
        (true, true) => review::overlay_sheet(sheet::with_sheet(dr, font), font),
        (true, false) => sheet::with_sheet(dr, font),
        (false, true) => review::overlay(dr, font),
        (false, false) => dr,
    };
    match format {
        "svg" => done(svg::render(&maybe_sheet(dr, want_sheet), font, &svg::Options::default())),
        "pdf" => done(pdf::render(&maybe_sheet(dr, true), font)),
        "dxf" => done(dxf::render(&maybe_sheet(dr, want_sheet))),
        "png" => {
            let mut opt = raster::Options::default();
            if let Some(pv) = inp.get("px") {
                let Some(n) = pv.num() else {
                    return fail(
                        "E_INPUT",
                        &format!(
                            "export \"px\" must be a number (output width in pixels, {}-{}, default {})",
                            raster::MIN_PX,
                            raster::MAX_PX,
                            raster::DEFAULT_PX
                        ),
                    );
                };
                let n = if n.is_nan() { 1600.0 } else { n };
                opt.px = crate::math::round(n).clamp(f64::from(raster::MIN_PX), f64::from(raster::MAX_PX)) as u32;
            }
            match raster::render(&maybe_sheet(dr, want_sheet), font, &opt) {
                Ok(b) => done(b),
                Err(e) => fail("E_INTERNAL", &format!("png encoding failed ({e:?})")),
            }
        }
        _ => fail("E_INPUT", &format!("export format must be \"svg\", \"dxf\", \"pdf\" or \"png\" (got \"{format}\")")),
    }
}

fn check_fn(inp: &Value) -> Output {
    let (d, st) = doc_and_style!(inp);
    let diags = Diags::new();
    let strict = matches!(inp.get("strict"), Some(Value::Bool(true)));
    let l = load::load_after_edit(d, &st, &diags, true, None, strict);
    if let Some(files) = inp.get("reference_files")
        && let Err(msg) = references::host_report(d, files, &diags)
    {
        return fail("E_INPUT", msg);
    }
    let mut out = Vec::from(&b"{\"diagnostics\":"[..]);
    json::write_compact(&mut out, &load::diags_json(&diags.list()));
    out.extend_from_slice(b",\"acknowledged\":");
    json::write_compact(&mut out, &lint::acked_json(&l.acked));
    out.extend_from_slice(b",\"summary\":");
    json::write_string(&mut out, &load::summary(&l));
    out.extend_from_slice(b"}\n");
    done(out)
}

fn inspect_fn(inp: &Value) -> Output {
    let (d, st) = doc_and_style!(inp);
    let diags = Diags::new();
    let l = load::load(d, &st, &diags, false);
    let empty = Value::Object(Vec::new());
    let q = inp.get("query").unwrap_or(&empty);
    match load::inspect(&l, q) {
        Ok(r) => {
            let mut out = Vec::new();
            json::write_compact(&mut out, &r);
            out.push(b'\n');
            done(out)
        }
        Err(e) => fail(e.code, &e.message),
    }
}

fn mesh_fn(inp: &Value) -> Output {
    let (d, st) = doc_and_style!(inp);
    let diags = Diags::new();
    let l = load::load(d, &st, &diags, false);
    let incl = matches!(inp.get("include_fills"), Some(Value::Bool(true)));
    done(mesh::to_json(&mesh::build(&l.scene, incl)))
}

fn apply_fn(inp: &Value) -> Output {
    let (d, st) = doc_and_style!(inp);
    let Some(ops_v) = inp.get("ops") else { return fail("E_INPUT", "missing \"ops\": an array of ops (see SPEC 14)") };
    let actor = match inp.get("actor").and_then(Value::str) {
        Some("designer") => Actor::Designer,
        _ => Actor::Llm,
    };
    let op_diags = Diags::new();
    let applied = ops::apply(d, ops_v, actor, &op_diags);
    let prior_diags = Diags::new();
    let mut all: Vec<Diag> = op_diags.into_vec();
    let mut ok = false;
    let mut final_doc = d;
    let mut changed: &[String] = &[];
    let mut acked = Vec::new();
    let summary_text;
    let after_diags = Diags::new();
    if let Some(ap) = &applied {
        let prior = load::load(d, &st, &prior_diags, false);
        let l = load::load_after_edit(&ap.doc, &st, &after_diags, true, Some(&prior), false);
        all.extend(after_diags.to_vec());
        if all.iter().all(|x| x.level != Level::Error) {
            ok = true;
            final_doc = &ap.doc;
            changed = &ap.changed;
            summary_text = load::summary_with(&l, &all);
            acked = l.acked;
        } else {
            summary_text = load::summary(&prior);
        }
    } else {
        let before = load::load(d, &st, &prior_diags, false);
        summary_text = load::summary(&before);
    }
    let mut out = Vec::from(if ok { &b"{\"ok\":true,\"doc\":"[..] } else { &b"{\"ok\":false,\"doc\":"[..] });
    let text = canon::write(final_doc);
    out.extend_from_slice(trim_newlines(&text));
    out.extend_from_slice(b",\"diagnostics\":");
    json::write_compact(&mut out, &load::diags_json(&all));
    out.extend_from_slice(b",\"acknowledged\":");
    json::write_compact(&mut out, &lint::acked_json(&acked));
    out.extend_from_slice(b",\"summary\":");
    json::write_string(&mut out, &summary_text);
    out.extend_from_slice(b",\"changed\":[");
    for (i, c) in changed.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        json::write_string(&mut out, c);
    }
    out.extend_from_slice(b"]}\n");
    done(out)
}
