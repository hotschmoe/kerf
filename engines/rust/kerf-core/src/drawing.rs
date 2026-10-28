//! Drawing IR (SPEC 10): the model-space output of a view, input of every exporter.

use crate::geom::{self, Pt};
use crate::json;
use crate::model::{self, Diag};
use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;

pub use crate::pen::Pen;

/// Which kind of view a drawing came from.
pub use crate::view::Kind;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub fn name(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VAlign {
    Baseline,
    Middle,
    Top,
}

impl VAlign {
    pub fn name(self) -> &'static str {
        match self {
            VAlign::Baseline => "baseline",
            VAlign::Middle => "middle",
            VAlign::Top => "top",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PathItem {
    pub layer: String,
    pub pen: Pen,
    pub src: String,
    pub closed: bool,
    pub pts: Vec<Pt>,
}

#[derive(Clone, Debug)]
pub struct FillItem {
    pub layer: String,
    pub src: String,
    pub loops: Vec<Vec<Pt>>,
}

#[derive(Clone, Debug)]
pub struct HatchItem {
    pub layer: String,
    pub pen: Pen,
    pub src: String,
    pub pattern: String,
    pub scale: f64,
    pub angle: f64,
    pub loops: Vec<Vec<Pt>>,
    pub lines: Vec<[f64; 4]>,
}

#[derive(Clone, Debug)]
pub struct TextItem {
    pub layer: String,
    pub pen: Pen,
    pub src: String,
    pub s: String,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub rot: f64,
    pub align: Align,
    pub valign: VAlign,
}

impl TextItem {
    pub fn new(layer: String, pen: Pen, src: String, s: String, x: f64, y: f64, h: f64) -> TextItem {
        TextItem { layer, pen, src, s, x, y, h, rot: 0.0, align: Align::Left, valign: VAlign::Baseline }
    }
}

/// Non-drawn picking region (SPEC 16): exact closed loops of a visible cut region / beyond face.
#[derive(Clone, Debug)]
pub struct RegionItem {
    pub src: String,
    pub part: Option<String>,
    pub instance: u32,
    pub cut: bool,
    pub loops: Vec<Vec<Pt>>,
    pub layer: String,
}

#[derive(Clone, Debug)]
pub enum Item {
    Region(RegionItem),
    Path(PathItem),
    Fill(FillItem),
    Hatch(HatchItem),
    Text(TextItem),
}

impl Item {
    pub fn src(&self) -> &str {
        match self {
            Item::Region(x) => &x.src,
            Item::Path(x) => &x.src,
            Item::Fill(x) => &x.src,
            Item::Hatch(x) => &x.src,
            Item::Text(x) => &x.src,
        }
    }

    pub fn layer(&self) -> &str {
        match self {
            Item::Region(x) => &x.layer,
            Item::Path(x) => &x.layer,
            Item::Fill(x) => &x.layer,
            Item::Hatch(x) => &x.layer,
            Item::Text(x) => &x.layer,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayerDef {
    pub name: String,
    pub lineweight_mm: f64,
    pub linetype: String,
}

#[derive(Clone, Debug)]
pub struct Drawing<'a> {
    pub doc: String,
    pub view: String,
    pub kind: Kind,
    pub scale: f64,
    pub bounds: [f64; 4],
    pub items: Vec<Item>,
    pub layers: Vec<LayerDef>,
    pub diagnostics: Vec<Diag>,
    pub style: &'a Style,

    // --- exporter extras (not part of the JSON IR) ---
    pub number: String,
    pub title: String,
    pub scale_label: String,
    pub sheet_no: String,
    pub date: String,
    pub code_basis: String,
    pub has_unverified: bool,
    /// The model-space crop window (section) or the projected window (iso).
    pub crop: geom::Box,
    /// Content extents excluding title items.
    pub detail_bounds: geom::Box,
    pub author: String,
    pub project: String,
    pub doc_title: String,
    /// Page size in paper inches when `bounds` is a full sheet (0 = not a sheet).
    pub page_w: f64,
    pub page_h: f64,
}

impl<'a> Drawing<'a> {
    pub fn new(doc: String, view: String, kind: Kind, scale: f64, bounds: [f64; 4], style: &'a Style) -> Drawing<'a> {
        Drawing {
            doc,
            view,
            kind,
            scale,
            bounds,
            items: Vec::new(),
            layers: Vec::new(),
            diagnostics: Vec::new(),
            style,
            number: String::new(),
            title: String::new(),
            scale_label: String::new(),
            sheet_no: String::new(),
            date: String::new(),
            code_basis: String::new(),
            has_unverified: false,
            crop: geom::Box::default(),
            detail_bounds: geom::Box::default(),
            author: String::new(),
            project: String::new(),
            doc_title: String::new(),
            page_w: 0.0,
            page_h: 0.0,
        }
    }
}

// ---- JSON output ----------------------------------------------------------------------------------------

fn raw(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

fn write_pts(out: &mut Vec<u8>, pts: &[Pt]) {
    out.push(b'[');
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.push(b'[');
        json::push_number(out, p.x);
        out.push(b',');
        json::push_number(out, p.y);
        if p.b != 0.0 {
            out.push(b',');
            // bulges keep extra precision
            let mut buf = [0u8; 40];
            raw(out, json::fmt_fixed(&mut buf, p.b, 6));
        }
        out.push(b']');
    }
    out.push(b']');
}

fn write_loops(out: &mut Vec<u8>, loops: &[Vec<Pt>]) {
    out.push(b'[');
    for (i, l) in loops.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        write_pts(out, l);
    }
    out.push(b']');
}

fn key_str(out: &mut Vec<u8>, key: &str, s: &str) {
    raw(out, key);
    json::write_string(out, s);
}

fn key_num(out: &mut Vec<u8>, key: &str, x: f64) {
    raw(out, key);
    json::push_number(out, x);
}

pub fn to_json(d: &Drawing) -> Vec<u8> {
    let mut out = Vec::new();
    raw(&mut out, "{\"kerf_drawing\":\"0.1\",\"doc\":");
    json::write_string(&mut out, &d.doc);
    key_str(&mut out, ",\"view\":", &d.view);
    key_str(&mut out, ",\"kind\":", d.kind.name());
    key_num(&mut out, ",\"scale\":", d.scale);
    raw(&mut out, ",\"bounds\":[");
    for (i, b) in d.bounds.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        json::push_number(&mut out, *b);
    }
    raw(&mut out, "],\n\"pens\":{");
    for (i, p) in d.style.pens.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        json::write_string(&mut out, &p.name);
        key_num(&mut out, ":{\"width_mm\":", p.width_mm);
        raw(&mut out, ",\"dash_mm\":");
        match &p.dash_mm {
            Some(dm) => {
                out.push(b'[');
                for (k, x) in dm.iter().enumerate() {
                    if k > 0 {
                        out.push(b',');
                    }
                    json::push_number(&mut out, *x);
                }
                out.push(b']');
            }
            None => raw(&mut out, "null"),
        }
        out.push(b'}');
    }
    raw(&mut out, "},\n\"layers\":[");
    for (i, l) in d.layers.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        raw(&mut out, "{\"name\":");
        json::write_string(&mut out, &l.name);
        key_num(&mut out, ",\"lineweight_mm\":", l.lineweight_mm);
        out.push(b'}');
    }
    raw(&mut out, "],\n\"items\":[\n");
    for (i, it) in d.items.iter().enumerate() {
        if i > 0 {
            raw(&mut out, ",\n");
        }
        match it {
            Item::Path(p) => {
                key_str(&mut out, "{\"t\":\"path\",\"layer\":", &p.layer);
                key_str(&mut out, ",\"pen\":", p.pen.name());
                key_str(&mut out, ",\"src\":", &p.src);
                raw(&mut out, if p.closed { ",\"closed\":true,\"pts\":" } else { ",\"closed\":false,\"pts\":" });
                write_pts(&mut out, &p.pts);
                out.push(b'}');
            }
            Item::Fill(f) => {
                key_str(&mut out, "{\"t\":\"fill\",\"layer\":", &f.layer);
                key_str(&mut out, ",\"src\":", &f.src);
                raw(&mut out, ",\"loops\":");
                write_loops(&mut out, &f.loops);
                out.push(b'}');
            }
            Item::Hatch(h) => {
                key_str(&mut out, "{\"t\":\"hatch\",\"layer\":", &h.layer);
                key_str(&mut out, ",\"pen\":", h.pen.name());
                key_str(&mut out, ",\"src\":", &h.src);
                key_str(&mut out, ",\"pattern\":", &h.pattern);
                key_num(&mut out, ",\"scale\":", h.scale);
                key_num(&mut out, ",\"angle\":", h.angle);
                raw(&mut out, ",\"loops\":");
                write_loops(&mut out, &h.loops);
                raw(&mut out, ",\"lines\":[");
                for (k, ln) in h.lines.iter().enumerate() {
                    if k > 0 {
                        out.push(b',');
                    }
                    out.push(b'[');
                    for (q, v) in ln.iter().enumerate() {
                        if q > 0 {
                            out.push(b',');
                        }
                        json::push_number(&mut out, *v);
                    }
                    out.push(b']');
                }
                raw(&mut out, "]}");
            }
            Item::Region(r) => {
                key_str(&mut out, "{\"t\":\"region\",\"src\":", &r.src);
                raw(&mut out, ",\"part\":");
                match &r.part {
                    Some(pp) => json::write_string(&mut out, pp),
                    None => raw(&mut out, "null"),
                }
                raw(&mut out, &alloc::format!(",\"instance\":{},\"cut\":{},\"loops\":", r.instance, r.cut));
                write_loops(&mut out, &r.loops);
                out.push(b'}');
            }
            Item::Text(t) => {
                key_str(&mut out, "{\"t\":\"text\",\"layer\":", &t.layer);
                key_str(&mut out, ",\"pen\":", t.pen.name());
                key_str(&mut out, ",\"src\":", &t.src);
                key_str(&mut out, ",\"s\":", &t.s);
                key_num(&mut out, ",\"x\":", t.x);
                key_num(&mut out, ",\"y\":", t.y);
                key_num(&mut out, ",\"h\":", t.h);
                key_num(&mut out, ",\"rot\":", t.rot);
                raw(&mut out, ",\"align\":\"");
                raw(&mut out, t.align.name());
                raw(&mut out, "\",\"valign\":\"");
                raw(&mut out, t.valign.name());
                raw(&mut out, "\"}");
            }
        }
    }
    raw(&mut out, "\n],\n\"diagnostics\":[");
    for (i, dg) in d.diagnostics.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        json::write_compact(&mut out, &model::diag_to_json(dg));
    }
    raw(&mut out, "]}\n");
    out
}
