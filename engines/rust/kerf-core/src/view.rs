//! View parameter parsing shared by section and iso views.

use crate::diag::Diag;
use crate::geom::Rect;
use crate::model::Model;
use crate::num::{parse_scale, scale_text};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct ViewParams {
    pub id: String,
    pub kind: String,
    pub number: String,
    pub title: String,
    pub scale_str: String,
    pub factor: Option<f64>,
    pub scale_text: String,
    pub cut_z: f64,
    pub crop: Option<Rect>,
    pub from: String,
    pub cutaway: bool,
    pub notes_side: String,
    pub omit: Vec<String>,
    pub annotations: Vec<Value>,
}

pub fn find_view<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    doc.get("views")?.as_array()?.iter().find(|v| v.get("id").and_then(|i| i.as_str()) == Some(id))
}

pub fn view_ids(doc: &Value) -> Vec<String> {
    doc.get("views")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

pub fn view_params(v: &Value, model: &Model) -> Result<ViewParams, Diag> {
    let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
    let kind = v.get("kind").and_then(|i| i.as_str()).unwrap_or("section").to_string();
    let scale_str = v.get("scale").and_then(|i| i.as_str()).unwrap_or("NTS").to_string();
    let factor = parse_scale(&scale_str).map_err(|e| Diag::error("E_PARAM", format!("views/{}/scale: {}", id, e)).path(format!("views/{}/scale", id)))?;
    let mid = (model.run.0 + model.run.1) * 0.5;
    let cut_z = v.get("cut_z").and_then(|c| c.as_f64()).unwrap_or(mid);
    let crop = v.get("crop").and_then(|c| {
        let x = c.get("x")?.as_array()?;
        let y = c.get("y")?.as_array()?;
        Some(Rect::new(x.first()?.as_f64()?, y.first()?.as_f64()?, x.get(1)?.as_f64()?, y.get(1)?.as_f64()?))
    });
    let notes_side = v.get("notes_side").and_then(|i| i.as_str()).unwrap_or("right").to_string();
    Ok(ViewParams {
        id,
        kind,
        number: v.get("number").map(|n| match n { Value::String(s) => s.clone(), other => crate::json::compact(other) }).unwrap_or_default(),
        title: v.get("title").and_then(|i| i.as_str()).unwrap_or("").to_string(),
        scale_text: scale_text(&scale_str, factor),
        scale_str,
        factor,
        cut_z,
        crop,
        from: v.get("from").and_then(|i| i.as_str()).unwrap_or("front_right").to_string(),
        cutaway: v.get("cutaway").and_then(|i| i.as_bool()).unwrap_or(false),
        notes_side,
        omit: v.get("omit").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default(),
        annotations: v.get("annotations").and_then(|a| a.as_array()).cloned().unwrap_or_default(),
    })
}
