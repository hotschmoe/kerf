//! View specs (SPEC 6): parsing and validation of `views[]`.

use crate::geom::Box;
use crate::json::Value;
use crate::limits;
use crate::model::{self, Diags, Level};
use crate::scene::Comp;
use crate::units;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Section view or isometric view; also the `kind` of the `Drawing` a view produces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Section,
    Iso,
    /// Tables and notes only (SPEC 26.1): no components, no crop, no model scale; laid out in paper inches.
    Schedule,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Section => "section",
            Kind::Iso => "iso",
            Kind::Schedule => "schedule",
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
    /// Keyed notes only: the DETAIL NOTES list goes above the drawing.
    Above,
}

/// How a view's notes are drawn (SPEC 24): text at the leader end, a numbered tag at the leader end with the text in a
/// DETAIL NOTES list, or the list alone (no tags, no leaders).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotesStyle {
    Leader,
    Keyed,
    List,
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
    /// `notes_style` of the view; None = the style's `notes.mode`.
    pub notes_style: Option<NotesStyle>,
    /// `notes_title`: the heading of the DETAIL NOTES list (None = the style's `notes.list_title`).
    pub notes_title: Option<&'a str>,
    pub annotations: &'a [Value],
    pub members: Members<'a>,
}

/// Which components a view draws (SPEC 25.4): a component's own `views` list, then the view's `only` and `omit`
/// patterns (component ids, `*` matching any run of characters).
#[derive(Clone, Debug, Default)]
pub struct Members<'a> {
    pub view: &'a str,
    pub omit: Vec<&'a str>,
    pub only: Option<Vec<&'a str>>,
    /// A schedule view draws no components.
    pub none: bool,
}

impl<'a> Members<'a> {
    pub fn of(node: &'a Value) -> Members<'a> {
        let pats = |k: &str| node.get(k).and_then(Value::arr).map(|a| a.iter().filter_map(Value::str).collect());
        Members {
            view: node.get("id").and_then(Value::str).unwrap_or(""),
            omit: pats("omit").unwrap_or_default(),
            only: pats("only"),
            none: node.get("kind").and_then(Value::str).is_some_and(is_schedule_kind),
        }
    }

    /// The component with this id and document node is drawn (and checked) in the view.
    pub fn includes(&self, id: &str, node: &Value) -> bool {
        if self.none {
            return false;
        }
        if let Some(vs) = node.get("views").and_then(Value::arr)
            && !vs.iter().any(|v| v.str() == Some(self.view))
        {
            return false;
        }
        if let Some(only) = &self.only
            && !only.iter().any(|p| glob_match(p, id))
        {
            return false;
        }
        !self.omit.iter().any(|p| glob_match(p, id))
    }

    pub fn draws(&self, c: &Comp<'_>) -> bool {
        self.includes(&c.id, c.node)
    }
}

/// `kind` of a tables-and-notes view: "schedule", or its alias "sheet".
pub fn is_schedule_kind(kind: &str) -> bool {
    matches!(kind, "schedule" | "sheet")
}

pub fn is_glob(pat: &str) -> bool {
    pat.contains('*')
}

/// `*` matches any run of characters (also none); everything else matches itself.
pub fn glob_match(pat: &str, s: &str) -> bool {
    let (p, t) = (pat.as_bytes(), s.as_bytes());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&b| b == b'*')
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

/// Keys of a schedule view that describe model geometry: ignored, each with a `W_PARAM` naming it.
fn schedule_only_keys(node: &Value, id: &str, base: &str, diags: &Diags) {
    for key in ["scale", "crop", "cut_z", "from", "cutaway", "omit", "only"] {
        let Some(v) = node.get(key).filter(|v| !v.is_null()) else { continue };
        if key == "scale" && v.str().is_some_and(|t| units::parse_scale(t).is_some_and(|s| s.factor == 0.0)) {
            continue;
        }
        diags.add_fix(
            Level::Warning,
            "W_PARAM",
            Some(id),
            Some(&format!("{base}/{key}")),
            format!("view {id} is a schedule view (tables and notes in paper inches, no components): \"{key}\" has no meaning there and is ignored; the caption prints SCALE: NTS"),
            format!("remove \"{key}\" from view {id} (\"scale\": \"NTS\" is accepted), or make it a \"section\" view to draw components"),
        );
    }
}

pub fn parse<'a>(node: &'a Value, index: usize, diags: &Diags) -> Option<ViewSpec<'a>> {
    let Some(id) = node.get("id").and_then(Value::str) else {
        diags.add(
            Level::Error,
            "E_PARAM",
            None,
            Some(&format!("views/{index}")),
            format!("view {index} needs a string \"id\""),
        );
        return None;
    };
    let base = format!("views/{id}");
    let mut ok = true;
    let kind_s = node.get("kind").and_then(Value::str).unwrap_or("section");
    let mut kind = Kind::Section;
    match kind_s {
        "section" => {}
        "iso" => kind = Kind::Iso,
        k if is_schedule_kind(k) => kind = Kind::Schedule,
        _ => {
            diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{base}/kind")),
                format!(
                    "view kind must be \"section\", \"iso\" or \"schedule\" (tables and notes only) (got \"{kind_s}\")"
                ),
            );
            ok = false;
        }
    }
    let mut has_scale = node.get("scale").is_some_and(|v| !v.is_null());
    let mut scale_text = node.get("scale").and_then(Value::str).unwrap_or(if kind == Kind::Section {
        DEFAULT_SECTION_SCALE
    } else {
        "NTS"
    });
    let mut scale = 12.0;
    let spath = format!("{base}/scale");
    if kind == Kind::Schedule {
        schedule_only_keys(node, id, &base, diags);
        has_scale = false;
        scale_text = "NTS";
        scale = 0.0;
    } else if let Some(s) = units::parse_scale(scale_text) {
        scale = s.factor;
        if kind == Kind::Section && s.factor == 0.0 {
            diags.add(Level::Error, "E_PARAM", Some(id), Some(&spath), "section views need a numeric scale such as \"1-1/2\\\"=1'-0\\\"\", \"1\\\"=1'-0\\\"\", \"3/4\\\"=1'-0\\\"\" or \"1:20\" (NTS is for iso views; a view of tables and notes only is \"kind\": \"schedule\", which needs no scale)".to_string());
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
    if let Some(cv) = node.get("crop").filter(|v| !v.is_null() && kind != Kind::Schedule) {
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
        if let Some(z) = units::parse_length(cz) {
            cut_z = z
        } else {
            diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{base}/cut_z")),
                "cut_z must be a length".to_string(),
            );
            ok = false;
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
                diags.add(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("{base}/from")),
                    format!("iso 'from' must be front_right, front_left, back_right or back_left (got \"{s}\")"),
                );
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
            "above" => notes_side = NotesSide::Above,
            _ => {
                diags.add(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("{base}/notes_side")),
                    format!("notes_side must be right, left, both or above (got \"{s}\")"),
                );
                ok = false;
            }
        }
    }
    let mut notes_style = None;
    if let Some(s) = node.get("notes_style").and_then(Value::str) {
        match s {
            "leader" => notes_style = Some(NotesStyle::Leader),
            "keyed" => notes_style = Some(NotesStyle::Keyed),
            "list" => notes_style = Some(NotesStyle::List),
            _ => {
                diags.add(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("{base}/notes_style")),
                    format!("notes_style must be leader, keyed or list (got \"{s}\")"),
                );
                ok = false;
            }
        }
    }
    let mut notes_title = None;
    if let Some(tv) = node.get("notes_title").filter(|v| !v.is_null()) {
        match tv.str().map(str::trim) {
            Some(t) if !t.is_empty() && t.chars().count() <= 60 => notes_title = Some(t),
            _ => {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("{base}/notes_title")),
                    format!(
                        "view {id}: \"notes_title\" must be a string of 1 to 60 characters (got {})",
                        model::kind_or_text(tv)
                    ),
                    "use e.g. \"notes_title\": \"HEADER SCHEDULE NOTES\", or remove it for DETAIL NOTES".to_string(),
                );
                ok = false;
            }
        }
    }
    let cutaway = matches!(node.get("cutaway"), Some(Value::Bool(true)));
    if !ok {
        return None;
    }
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
        notes_style,
        notes_title,
        annotations: anns,
        members: Members::of(node),
    })
}
