//! Kerf Style (SPEC section 7): parsed from JSON, with the standard style embedded.

use serde_json::{Map, Value};
use std::sync::OnceLock;

pub const DEFAULT_STYLE_JSON: &str = include_str!("../../../../spec/styles/kerf-standard.kerfstyle.json");

#[derive(Clone, Debug)]
pub struct Pen {
    pub name: String,
    pub width_mm: f64,
    pub dash_mm: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct HatchSpec {
    pub pattern: String,
    pub scale: f64,
    pub angle: f64,
}

#[derive(Clone, Debug)]
pub struct MatStyle {
    pub name: String,
    pub hatch: Vec<HatchSpec>,
    pub cut_mark: Option<String>,
    pub color3d: String,
    pub fill: bool,
    pub batt: bool,
    pub pen: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PatLine {
    pub angle: f64,
    pub x0: f64,
    pub y0: f64,
    pub dx: f64,
    pub dy: f64,
    pub dashes: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct LayerStyle {
    pub key: String,
    pub name: String,
    pub lineweight_mm: f64,
    pub linetype: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Style {
    pub id: String,
    pub pens: Vec<Pen>,
    pub materials: Vec<MatStyle>,
    pub patterns: Vec<(String, Vec<PatLine>)>,
    pub layers: Vec<LayerStyle>,
    // text
    pub text_height: f64,
    pub title_height: f64,
    pub label_height: f64,
    pub text_upper: bool,
    pub line_spacing: f64,
    pub dxf_style: String,
    pub dxf_font: String,
    // notes
    pub notes_mode: String,
    pub wrap_chars: usize,
    pub gutter: f64,
    pub shoulder: f64,
    pub note_gap: f64,
    pub arrow: String,
    pub arrow_len: f64,
    pub arrow_width: f64,
    // dims
    pub dim_terminator: String,
    pub tick_len: f64,
    pub ext_gap: f64,
    pub ext_over: f64,
    pub text_gap: f64,
    pub precision: f64,
    // citations
    pub cite_format: String,
    pub cite_unverified: String,
    pub cite_flag: String,
    pub cite_footnote: String,
    // sheet
    pub sheet_w: f64,
    pub sheet_h: f64,
    pub margin: f64,
    pub title_block_h: f64,
    pub firm: String,
    pub project: String,
    // break line
    pub brk_zig: f64,
    pub brk_period: f64,
    pub brk_over: f64,
    // 3d colors
    pub bg3d: String,
    pub edge3d: String,
    pub selection3d: String,
    pub cut_cap3d: String,
}

static DEFAULT: OnceLock<Style> = OnceLock::new();

pub fn default_style() -> &'static Style {
    DEFAULT.get_or_init(|| {
        let v: Value = serde_json::from_str(DEFAULT_STYLE_JSON).expect("embedded style is valid JSON");
        Style::from_value(&v).expect("embedded style parses")
    })
}

pub fn default_style_value() -> Value {
    serde_json::from_str(DEFAULT_STYLE_JSON).expect("embedded style is valid JSON")
}

/// Deep merge: objects merge key-wise, everything else is replaced.
pub fn merge(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Object(a), Value::Object(b)) => {
            let mut m: Map<String, Value> = a.clone();
            for (k, v) in b {
                let nv = match m.get(k) {
                    Some(old) => merge(old, v),
                    None => v.clone(),
                };
                m.insert(k.clone(), nv);
            }
            Value::Object(m)
        }
        (_, o) => o.clone(),
    }
}

fn f(v: &Value, path: &[&str], d: f64) -> f64 {
    let mut cur = v;
    for p in path {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return d,
        }
    }
    cur.as_f64().unwrap_or(d)
}

fn st(v: &Value, path: &[&str], d: &str) -> String {
    let mut cur = v;
    for p in path {
        match cur.get(p) {
            Some(x) => cur = x,
            None => return d.to_string(),
        }
    }
    cur.as_str().unwrap_or(d).to_string()
}

impl Style {
    /// Parse a style value; the user's style is merged over the embedded default first.
    pub fn from_user(user: &Value) -> Result<Style, String> {
        if user.is_null() {
            return Ok(default_style().clone());
        }
        let merged = merge(&default_style_value(), user);
        Style::from_value(&merged)
    }

    pub fn from_value(v: &Value) -> Result<Style, String> {
        if !v.is_object() {
            return Err("style must be a JSON object (a .kerfstyle.json document)".into());
        }
        let mut pens = vec![];
        if let Some(m) = v.get("pens").and_then(|x| x.as_object()) {
            for (k, p) in m {
                let dash = p.get("dash_mm").and_then(|d| d.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect::<Vec<f64>>());
                pens.push(Pen { name: k.clone(), width_mm: p.get("width_mm").and_then(|x| x.as_f64()).unwrap_or(0.25), dash_mm: dash });
            }
        }
        let mut materials = vec![];
        if let Some(m) = v.get("materials").and_then(|x| x.as_object()) {
            for (k, mv) in m {
                let mut hatch = vec![];
                if let Some(a) = mv.get("hatch").and_then(|x| x.as_array()) {
                    for h in a {
                        hatch.push(HatchSpec {
                            pattern: st(h, &["pattern"], "ANSI31"),
                            scale: f(h, &["scale"], 1.0),
                            angle: f(h, &["angle"], 0.0),
                        });
                    }
                }
                materials.push(MatStyle {
                    name: k.clone(),
                    hatch,
                    cut_mark: mv.get("cut_mark").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    color3d: st(mv, &["color3d"], "#A0A0A0"),
                    fill: mv.get("fill").and_then(|x| x.as_bool()).unwrap_or(false),
                    batt: mv.get("batt").and_then(|x| x.as_bool()).unwrap_or(false),
                    pen: mv.get("pen").and_then(|x| x.as_str()).map(|s| s.to_string()),
                });
            }
        }
        let mut patterns = vec![];
        if let Some(m) = v.get("patterns").and_then(|x| x.as_object()) {
            for (k, pv) in m {
                if k.starts_with('_') {
                    continue;
                }
                let mut lines = vec![];
                if let Some(a) = pv.as_array() {
                    for l in a {
                        let nums: Vec<f64> = l.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
                        if nums.len() >= 5 {
                            lines.push(PatLine { angle: nums[0], x0: nums[1], y0: nums[2], dx: nums[3], dy: nums[4], dashes: nums[5..].to_vec() });
                        }
                    }
                }
                patterns.push((k.clone(), lines));
            }
        }
        let mut layers = vec![];
        if let Some(m) = v.get("layers").and_then(|x| x.as_object()) {
            for (k, lv) in m {
                layers.push(LayerStyle {
                    key: k.clone(),
                    name: st(lv, &["name"], k),
                    lineweight_mm: f(lv, &["lineweight_mm"], 0.25),
                    linetype: lv.get("linetype").and_then(|x| x.as_str()).map(|s| s.to_string()),
                });
            }
        }
        Ok(Style {
            id: st(v, &["id"], "style"),
            pens,
            materials,
            patterns,
            layers,
            text_height: f(v, &["text", "height_in"], 0.09375),
            title_height: f(v, &["text", "title_height_in"], 0.15625),
            label_height: f(v, &["text", "label_height_in"], 0.078125),
            text_upper: st(v, &["text", "case"], "upper") == "upper",
            line_spacing: f(v, &["text", "line_spacing"], 1.6),
            dxf_style: st(v, &["text", "dxf_style"], "KERF"),
            dxf_font: st(v, &["text", "dxf_font"], "romans.shx"),
            notes_mode: st(v, &["notes", "mode"], "leader"),
            wrap_chars: f(v, &["notes", "wrap_chars"], 28.0) as usize,
            gutter: f(v, &["notes", "gutter_in"], 0.375),
            shoulder: f(v, &["notes", "shoulder_in"], 0.125),
            note_gap: f(v, &["notes", "note_gap_in"], 0.0625),
            arrow: st(v, &["notes", "arrow"], "closed_filled"),
            arrow_len: f(v, &["notes", "arrow_len_in"], 0.09375),
            arrow_width: f(v, &["notes", "arrow_width_in"], 0.03125),
            dim_terminator: st(v, &["dims", "terminator"], "tick"),
            tick_len: f(v, &["dims", "tick_len_in"], 0.0625),
            ext_gap: f(v, &["dims", "ext_gap_in"], 0.0625),
            ext_over: f(v, &["dims", "ext_over_in"], 0.0625),
            text_gap: f(v, &["dims", "text_gap_in"], 0.046875),
            precision: f(v, &["dims", "precision"], 16.0),
            cite_format: st(v, &["citations", "format"], " ({code} {section})"),
            cite_unverified: st(v, &["citations", "unverified"], "flag"),
            cite_flag: st(v, &["citations", "flag"], "*"),
            cite_footnote: st(v, &["citations", "footnote"], "* CODE REFERENCE NOT VERIFIED BY DESIGNER"),
            sheet_w: v["sheet"]["size_in"][0].as_f64().unwrap_or(11.0),
            sheet_h: v["sheet"]["size_in"][1].as_f64().unwrap_or(8.5),
            margin: f(v, &["sheet", "margin_in"], 0.375),
            title_block_h: f(v, &["sheet", "title_block_height_in"], 0.75),
            firm: st(v, &["sheet", "firm"], ""),
            project: st(v, &["sheet", "project"], ""),
            brk_zig: f(v, &["break_line", "zig_in"], 0.125),
            brk_period: f(v, &["break_line", "period_in"], 0.5),
            brk_over: f(v, &["break_line", "overshoot_in"], 0.0625),
            bg3d: st(v, &["colors3d", "background"], "#F2EFE6"),
            edge3d: st(v, &["colors3d", "edge"], "#1A1A1A"),
            selection3d: st(v, &["colors3d", "selection"], "#C8102E"),
            cut_cap3d: st(v, &["colors3d", "cut_cap"], "#E9D9A6"),
        })
    }

    pub fn pen(&self, name: &str) -> Pen {
        self.pens
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .unwrap_or(Pen { name: name.to_string(), width_mm: 0.25, dash_mm: None })
    }

    pub fn pen_width_in(&self, name: &str) -> f64 {
        self.pen(name).width_mm / 25.4
    }

    pub fn material(&self, name: &str) -> MatStyle {
        self.materials.iter().find(|m| m.name == name).cloned().unwrap_or_else(|| {
            self.materials.iter().find(|m| m.name == "generic").cloned().unwrap_or(MatStyle {
                name: name.to_string(),
                hatch: vec![],
                cut_mark: None,
                color3d: "#A0A0A0".into(),
                fill: false,
                batt: false,
                pen: None,
            })
        })
    }

    pub fn has_material(&self, name: &str) -> bool {
        self.materials.iter().any(|m| m.name == name)
    }

    pub fn pattern(&self, name: &str) -> Option<&Vec<PatLine>> {
        self.patterns.iter().find(|(n, _)| n == name).map(|(_, l)| l)
    }

    pub fn layer(&self, key: &str) -> LayerStyle {
        self.layers.iter().find(|l| l.key == key).cloned().unwrap_or(LayerStyle {
            key: key.to_string(),
            name: key.to_string(),
            lineweight_mm: 0.25,
            linetype: None,
        })
    }
}
