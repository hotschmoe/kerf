//! View specs (SPEC 6): parsing and validation of `views[]`.

use crate::geom::Box;
use crate::json::Value;
use crate::limits;
use crate::model::{self, Diags, Level};
use crate::units;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Section view or isometric view; also the `kind` of the `Drawing` a view produces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Section,
    Iso,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Section => "section",
            Kind::Iso => "iso",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum From {
    FrontRight,
    FrontLeft,
    BackRight,
    BackLeft,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotesSide {
    Right,
    Left,
    Both,
}

#[derive(Clone, Debug)]
pub struct ViewSpec<'a> {
    pub id: &'a str,
    pub kind: Kind,
    pub number: String,
    pub title: &'a str,
    pub scale_text: &'a str,
    /// Model inches per paper inch; 0 = NTS (fit).
    pub scale: f64,
    pub cut_z: f64,
    pub crop: Box,
    /// The author set `crop` / `scale` explicitly (SPEC 19). When false, drawview auto-fits them.
    pub has_crop: bool,
    pub has_scale: bool,
    pub from: From,
    pub cutaway: bool,
    pub notes_side: NotesSide,
    pub annotations: &'a [Value],
    pub omit: Vec<&'a str>,
    pub node: &'a Value,
}

pub fn find_view<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    doc.get("views")?.arr()?.iter().find(|v| v.get("id").and_then(Value::str) == Some(id))
}

pub fn view_ids(doc: &Value) -> Vec<&str> {
    let Some(vs) = doc.get("views").and_then(Value::arr) else { return Vec::new() };
    vs.iter().filter_map(|v| v.get("id").and_then(Value::str)).collect()
}

fn parse_range(v: Option<&Value>) -> Option<[f64; 2]> {
    let [a, b] = v?.arr()? else { return None };
    Some([units::parse_length(a)?, units::parse_length(b)?])
}

const DEFAULT_SECTION_SCALE: &str = "1\"=1'-0\"";

pub fn parse<'a>(node: &'a Value, index: usize, diags: &Diags) -> Option<ViewSpec<'a>> {
    let Some(id) = node.get("id").and_then(Value::str) else {
        diags.add(Level::Error, "E_PARAM", None, Some(&format!("views/{index}")), format!("view {index} needs a string \"id\""));
        return None;
    };
    let base = format!("views/{id}");
    let mut ok = true;
    let kind_s = node.get("kind").and_then(Value::str).unwrap_or("section");
    let mut kind = Kind::Section;
    match kind_s {
        "section" => {}
        "iso" => kind = Kind::Iso,
        _ => {
            diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{base}/kind")), format!("view kind must be \"section\" or \"iso\" (got \"{kind_s}\")"));
            ok = false;
        }
    }
    let has_scale = node.get("scale").is_some_and(|v| !v.is_null());
    let scale_text = node.get("scale").and_then(Value::str).unwrap_or(if kind == Kind::Iso { "NTS" } else { DEFAULT_SECTION_SCALE });
    let mut scale = 12.0;
    let spath = format!("{base}/scale");
    if let Some(s) = units::parse_scale(scale_text) {
        scale = s.factor;
        if kind == Kind::Section && s.factor == 0.0 {
            diags.add(Level::Error, "E_PARAM", Some(id), Some(&spath), "section views need a numeric scale such as \"1-1/2\\\"=1'-0\\\"\", \"1\\\"=1'-0\\\"\", \"3/4\\\"=1'-0\\\"\" or \"1:20\" (NTS is for iso views)".to_string());
            ok = false;
        }
    } else if let Some(s) = units::parse_scale_any(scale_text) {
        diags.add(Level::Error, "E_PARAM", Some(id), Some(&spath), format!("scale \"{scale_text}\" means {} model inches per paper inch; the supported range is {} to {} (from a 10x enlargement to 1:10000). Use e.g. \"3\\\"=1'-0\\\"\" (4), \"1\\\"=1'-0\\\"\" (12) or \"1:20\"", model::num_text(s.factor), model::fmt_num(units::MIN_SCALE_FACTOR), model::fmt_num(units::MAX_SCALE_FACTOR)));
        ok = false;
    } else {
        diags.add(Level::Error, "E_PARAM", Some(id), Some(&spath), format!("unrecognised scale \"{scale_text}\". Use e.g. \"3\\\"=1'-0\\\"\", \"1-1/2\\\"=1'-0\\\"\", \"1\\\"=1'-0\\\"\", \"3/4\\\"=1'-0\\\"\", \"1/2\\\"=1'-0\\\"\", \"3/8\\\"=1'-0\\\"\", \"1/4\\\"=1'-0\\\"\", \"1:N\" or \"NTS\""));
        ok = false;
    }
    let mut crop = Box::default();
    let mut has_crop = false;
    if let Some(cv) = node.get("crop").filter(|v| !v.is_null()) {
        match (parse_range(cv.get("x")), parse_range(cv.get("y"))) {
            (Some([x0, x1]), Some([y0, y1])) if x0 < x1 && y0 < y1 => {
                crop = Box { x0, y0, x1, y1 };
                has_crop = true;
            }
            _ => {
                diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{base}/crop")), format!("crop must be {{\"x\": [x0, x1], \"y\": [y0, y1]}} with x0 < x1 and y0 < y1, every value a length of at most {} inches in magnitude (got {})", model::fmt_num(limits::MAX_COORD_IN), model::kind_or_text(cv)));
                ok = false;
            }
        }
    }
    let mut cut_z = 0.0;
    if let Some(cz) = node.get("cut_z").filter(|v| !v.is_null()) {
        match units::parse_length(cz) {
            Some(z) => cut_z = z,
            None => {
                diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{base}/cut_z")), "cut_z must be a length".to_string());
                ok = false;
            }
        }
    }
    let mut from = From::FrontRight;
    if let Some(s) = node.get("from").and_then(Value::str) {
        match s {
            "front_right" => from = From::FrontRight,
            "front_left" => from = From::FrontLeft,
            "back_right" => from = From::BackRight,
            "back_left" => from = From::BackLeft,
            _ => {
                diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{base}/from")), format!("iso 'from' must be front_right, front_left, back_right or back_left (got \"{s}\")"));
                ok = false;
            }
        }
    }
    let mut notes_side = NotesSide::Both;
    if let Some(s) = node.get("notes_side").and_then(Value::str) {
        match s {
            "right" => notes_side = NotesSide::Right,
            "left" => notes_side = NotesSide::Left,
            "both" => {}
            _ => {
                diags.add(Level::Error, "E_PARAM", Some(id), Some(&format!("{base}/notes_side")), format!("notes_side must be right, left or both (got \"{s}\")"));
                ok = false;
            }
        }
    }
    let cutaway = matches!(node.get("cutaway"), Some(Value::Bool(true)));
    if !ok {
        return None;
    }
    let omit = node.get("omit").and_then(Value::arr).map(|oa| oa.iter().filter_map(Value::str).collect()).unwrap_or_default();
    let anns = node.get("annotations").map_or(&[][..], |av| av.arr().unwrap_or(&[]));
    if anns.len() > limits::MAX_ANNOTATIONS_PER_VIEW {
        let msg = limits::message(
            &format!("annotations (notes, dims, labels) in view {id}"),
            anns.len(),
            limits::MAX_ANNOTATIONS_PER_VIEW,
            "Nothing was laid out: the router cost grows with the square of the note count.",
        );
        diags.add_fix(Level::Error, "E_LIMIT", Some(id), Some(&format!("{base}/annotations")), msg, "keep the most important notes, or move part of the detail to a second view (a new entry in views[] with its own crop)".to_string());
        return None;
    }
    Some(ViewSpec {
        id,
        kind,
        number: match node.get("number") {
            Some(x) => x.str().unwrap_or("").to_string(),
            None => (index + 1).to_string(),
        },
        title: node.get("title").map_or("", |x| x.str().unwrap_or("")),
        scale_text,
        scale,
        cut_z,
        crop,
        has_crop,
        has_scale,
        from,
        cutaway,
        notes_side,
        annotations: anns,
        omit,
        node,
    })
}
