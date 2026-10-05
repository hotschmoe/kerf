//! The engine API shared by the CLI, the wasm ABI and in-process callers (SPEC section 13).

use crate::diag::{Diag, Level, count, diags_json};
use crate::json;
use crate::model::Model;
use crate::style::Style;
use serde_json::{Map, Value};

pub enum Output {
    Json(String),
    Bytes(Vec<u8>),
}

impl Output {
    pub fn bytes(self) -> Vec<u8> {
        match self {
            Output::Json(s) => s.into_bytes(),
            Output::Bytes(b) => b,
        }
    }
}

pub struct Loaded {
    pub canon: Value,
    pub model: Model,
    pub style: Style,
    pub diags: Vec<Diag>,
}

fn doc_of(input: &Value) -> Result<Value, String> {
    match input.get("doc") {
        Some(Value::String(s)) => json::parse(s),
        Some(v @ Value::Object(_)) => Ok(v.clone()),
        Some(_) => Err("\"doc\" must be a Kerf document object (or its JSON text)".into()),
        None => Err("missing \"doc\": pass the Kerf document JSON as {\"doc\": {...}}".into()),
    }
}

fn style_of(input: &Value) -> Result<Style, String> {
    match input.get("style") {
        None | Some(Value::Null) => Ok(crate::style::default_style().clone()),
        Some(Value::String(s)) if s.is_empty() || s == "default" || s == "kerf-standard" => Ok(crate::style::default_style().clone()),
        Some(Value::String(s)) => Style::from_user(&json::parse(s)?),
        Some(v) => Style::from_user(v),
    }
}

/// Canonicalize, compile and validate a document.
pub fn load(doc: &Value, style: Style) -> Loaded {
    let mut diags: Vec<Diag> = vec![];
    let canon = crate::schema::canon_doc(doc, &mut diags);
    let model = crate::resolve::compile(&canon, &style, &mut diags);
    crate::validate::validate(&canon, &model, &style, &mut diags);
    Loaded { canon, model, style, diags }
}

pub fn nviews(canon: &Value) -> usize {
    canon.get("views").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0)
}

pub fn summary_of(l: &Loaded) -> String {
    crate::summary::summary(&l.model, nviews(&l.canon), &l.diags)
}

pub fn version_json() -> Value {
    json::obj(vec![("engine", json::s(crate::ENGINE)), ("version", json::s(crate::VERSION)), ("spec", json::s(crate::SPEC))])
}

pub fn err_json(code: &str, message: &str) -> String {
    json::compact(&json::obj(vec![("error", json::obj(vec![("code", json::s(code)), ("message", json::s(message))]))]))
}

/// Dispatch one API call. `Err` carries a message; callers wrap it as error JSON.
pub fn call(fn_name: &str, input: &str) -> Result<Output, String> {
    let inp: Value = if input.trim().is_empty() { Value::Object(Map::new()) } else { json::parse(input)? };
    match fn_name {
        "version" => Ok(Output::Json(json::compact(&version_json()))),
        "catalog" => {
            let fmt = inp.get("format").and_then(|f| f.as_str()).unwrap_or("json");
            Ok(Output::Json(match fmt {
                "markdown" | "md" => json::compact(&Value::String(crate::catalog::markdown())),
                _ => json::compact(&crate::catalog::json()),
            }))
        }
        "fmt" => {
            let doc = doc_of(&inp)?;
            let mut errs = vec![];
            let canon = crate::schema::canon_doc(&doc, &mut errs);
            let errs: Vec<Diag> = errs.into_iter().filter(|d| d.level == Level::Error).collect();
            if !errs.is_empty() {
                return Err(format!("document has errors, cannot canonicalize: {}", errs.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join(" | ")));
            }
            let text = json::pretty(&canon);
            Ok(Output::Json(json::compact(&json::obj(vec![("doc", canon), ("text", Value::String(text))]))))
        }
        "check" => {
            let l = load(&doc_of(&inp)?, style_of(&inp)?);
            Ok(Output::Json(json::compact(&json::obj(vec![("diagnostics", diags_json(&l.diags)), ("summary", Value::String(summary_of(&l)))]))))
        }
        "drawing" => {
            let l = load(&doc_of(&inp)?, style_of(&inp)?);
            let view = inp.get("view").and_then(|v| v.as_str()).ok_or("missing \"view\": the view id, e.g. \"A\"")?;
            let d = crate::render::render_view(&l.canon, &l.model, view, &l.style).map_err(|d| d.message + &d.fix.map(|f| format!(" Fix: {}", f)).unwrap_or_default())?;
            Ok(Output::Json(json::compact(&d.to_json(&l.style))))
        }
        "export" => {
            let l = load(&doc_of(&inp)?, style_of(&inp)?);
            let view = inp.get("view").and_then(|v| v.as_str()).ok_or("missing \"view\": the view id, e.g. \"A\"")?;
            let format = inp.get("format").and_then(|v| v.as_str()).unwrap_or("svg");
            let sheet = inp.get("sheet").and_then(|v| v.as_bool()).unwrap_or(false);
            let d = crate::render::render_view(&l.canon, &l.model, view, &l.style).map_err(|d| d.message + &d.fix.map(|f| format!(" Fix: {}", f)).unwrap_or_default())?;
            match format {
                "svg" => Ok(Output::Bytes(crate::export_svg::export_svg(&d, &l.style, sheet).into_bytes())),
                other => Err(format!("unsupported export format \"{}\": use svg, dxf or pdf", other)),
            }
        }
        other => Err(format!("unknown function \"{}\": use version, catalog, fmt, check, apply, inspect, drawing, mesh, export", other)),
    }
}

#[allow(dead_code)]
fn counts(d: &[Diag]) -> (usize, usize, usize) {
    count(d)
}
