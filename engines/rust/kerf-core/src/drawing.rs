//! Drawing IR (SPEC section 10).

use crate::diag::{Diag, diags_json};
use crate::font::font;
use crate::geom::*;
use crate::json::{arr_nums, num, obj, s};
use crate::style::Style;
use serde_json::{Map, Value};

#[derive(Clone, Debug)]
pub enum Item {
    Path { layer: String, pen: String, src: String, closed: bool, pts: Vec<V> },
    Fill { layer: String, src: String, loops: Vec<Vec<V>> },
    Hatch { layer: String, pen: String, src: String, pattern: String, scale: f64, angle: f64, loops: Vec<Vec<V>>, lines: Vec<[f64; 4]> },
    Text { layer: String, pen: String, src: String, s: String, x: f64, y: f64, h: f64, rot: f64, align: String, valign: String },
}

impl Item {
    pub fn src(&self) -> &str {
        match self {
            Item::Path { src, .. } | Item::Fill { src, .. } | Item::Hatch { src, .. } | Item::Text { src, .. } => src,
        }
    }
    pub fn layer(&self) -> &str {
        match self {
            Item::Path { layer, .. } | Item::Fill { layer, .. } | Item::Hatch { layer, .. } | Item::Text { layer, .. } => layer,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SheetInfo {
    pub number: String,
    pub title: String,
    pub scale_text: String,
    pub sheet: String,
    pub date: String,
    pub author: String,
    pub project: String,
    pub code_basis: String,
    pub unverified: bool,
    pub doc_title: String,
}

#[derive(Clone, Debug)]
pub struct Drawing {
    pub doc: String,
    pub view: String,
    pub kind: String,
    pub scale: f64,
    pub bounds: Rect,
    pub crop: Rect,
    pub items: Vec<Item>,
    pub diagnostics: Vec<Diag>,
    pub info: SheetInfo,
}

pub fn pen_layer_key(pen: &str) -> &'static str {
    match pen {
        "cut" | "profile" | "membrane" | "vapor" => "cut",
        "beyond" => "beyond",
        "hidden" => "hidden",
        "hatch" => "hatch",
        "rebar" | "steel" => "steel",
        "anno" => "notes",
        "dim" => "dims",
        "break" => "break",
        "title" | "frame" => "title",
        _ => "beyond",
    }
}

pub fn text_width(text: &str, h: f64) -> f64 {
    font().width(text, h)
}

fn item_bounds(it: &Item, r: &mut Rect) {
    match it {
        Item::Path { pts, closed, .. } => {
            let mut b = Rect::empty();
            if pts.len() >= 2 {
                let segs = if *closed { loop_segs(&pts.to_vec()) } else { poly_segs(pts) };
                for sg in segs {
                    b.union(&sg.bbox());
                }
            } else {
                for p in pts {
                    b.add(p.p());
                }
            }
            r.union(&b);
        }
        Item::Fill { loops, .. } | Item::Hatch { loops, .. } => {
            for l in loops {
                r.union(&loop_bbox(l));
            }
        }
        Item::Text { s, x, y, h, rot, align, valign, .. } => {
            let w = text_width(s, *h);
            let ox = match align.as_str() {
                "center" => -w * 0.5,
                "right" => -w,
                _ => 0.0,
            };
            let oy = match valign.as_str() {
                "middle" => -h * 0.5,
                "top" => -h,
                _ => 0.0,
            };
            let ang = rot.to_radians();
            for (px, py) in [(ox, oy), (ox + w, oy), (ox + w, oy + h), (ox, oy + h)] {
                let q = pt(px, py).rot(ang);
                r.add(pt(x + q.x, y + q.y));
            }
        }
    }
}

pub fn items_bounds(items: &[Item]) -> Rect {
    let mut r = Rect::empty();
    for it in items {
        item_bounds(it, &mut r);
    }
    r
}

impl Drawing {
    pub fn compute_bounds(&mut self) {
        let mut r = self.crop;
        for it in &self.items {
            item_bounds(it, &mut r);
        }
        self.bounds = r;
    }

    pub fn to_json(&self, style: &Style) -> Value {
        let mut m = Map::new();
        m.insert("kerf_drawing".into(), s("0.1"));
        m.insert("doc".into(), s(&self.doc));
        m.insert("view".into(), s(&self.view));
        m.insert("kind".into(), s(&self.kind));
        m.insert("scale".into(), num(self.scale));
        m.insert("bounds".into(), arr_nums(&[self.bounds.x0, self.bounds.y0, self.bounds.x1, self.bounds.y1]));
        let mut pens = Map::new();
        for p in &style.pens {
            let dash = match &p.dash_mm {
                Some(d) => arr_nums(d),
                None => Value::Null,
            };
            pens.insert(p.name.clone(), obj(vec![("width_mm", num(p.width_mm)), ("dash_mm", dash)]));
        }
        m.insert("pens".into(), Value::Object(pens));
        let mut layers = vec![];
        for l in used_layers(self, style) {
            layers.push(obj(vec![("name", s(&l.name)), ("lineweight_mm", num(l.lineweight_mm))]));
        }
        m.insert("layers".into(), Value::Array(layers));
        let items: Vec<Value> = self.items.iter().map(item_json).collect();
        m.insert("items".into(), Value::Array(items));
        m.insert("diagnostics".into(), diags_json(&self.diagnostics));
        Value::Object(m)
    }
}

pub fn loop_json(l: &[V]) -> Value {
    Value::Array(
        l.iter()
            .map(|p| if p.b != 0.0 { arr_nums(&[p.x, p.y, p.b]) } else { arr_nums(&[p.x, p.y, 0.0]) })
            .collect(),
    )
}

fn item_json(it: &Item) -> Value {
    match it {
        Item::Path { layer, pen, src, closed, pts } => obj(vec![
            ("t", s("path")),
            ("layer", s(layer)),
            ("pen", s(pen)),
            ("src", s(src)),
            ("closed", Value::Bool(*closed)),
            ("pts", loop_json(pts)),
        ]),
        Item::Fill { layer, src, loops } => obj(vec![
            ("t", s("fill")),
            ("layer", s(layer)),
            ("src", s(src)),
            ("loops", Value::Array(loops.iter().map(|l| loop_json(l)).collect())),
        ]),
        Item::Hatch { layer, pen, src, pattern, scale, angle, loops, lines } => obj(vec![
            ("t", s("hatch")),
            ("layer", s(layer)),
            ("pen", s(pen)),
            ("src", s(src)),
            ("pattern", s(pattern)),
            ("scale", num(*scale)),
            ("angle", num(*angle)),
            ("loops", Value::Array(loops.iter().map(|l| loop_json(l)).collect())),
            ("lines", Value::Array(lines.iter().map(|l| arr_nums(l)).collect())),
        ]),
        Item::Text { layer, pen, src, s: text, x, y, h, rot, align, valign } => obj(vec![
            ("t", s("text")),
            ("layer", s(layer)),
            ("pen", s(pen)),
            ("src", s(src)),
            ("s", s(text)),
            ("x", num(*x)),
            ("y", num(*y)),
            ("h", num(*h)),
            ("rot", num(*rot)),
            ("align", s(align)),
            ("valign", s(valign)),
        ]),
    }
}

/// Layers referenced by the drawing's items, in style order.
pub fn used_layers(d: &Drawing, style: &Style) -> Vec<crate::style::LayerStyle> {
    let mut names: Vec<&str> = vec![];
    for it in &d.items {
        let l = it.layer();
        if !names.contains(&l) {
            names.push(l);
        }
    }
    let mut out = vec![];
    for ls in &style.layers {
        if names.contains(&ls.name.as_str()) {
            out.push(ls.clone());
        }
    }
    for n in names {
        if !out.iter().any(|l| l.name == n) {
            out.push(crate::style::LayerStyle { key: n.to_string(), name: n.to_string(), lineweight_mm: 0.25, linetype: None });
        }
    }
    out
}

pub fn layer_name(style: &Style, key: &str) -> String {
    style.layer(key).name
}
