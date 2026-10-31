//! Core model types shared by builders, the scene compiler, views and exporters.
//!
//! `Diags` is shared by `&` everywhere (scene, builder parameter readers, validators append to the same list in call order), so
//! its list sits behind a `RefCell`; nothing holds a borrow across a call.

use crate::geom::{self, Pt, V2, Xf};
use crate::json::{self, Value};
use crate::limits;
use crate::num;
use crate::pen::Pen;
use crate::style::Role;
use crate::units;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Ref, RefCell};

// ---- diagnostics -----------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Error,
    Warning,
    Info,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub level: Level,
    pub code: &'static str,
    pub id: Option<String>,
    pub path: Option<String>,
    pub message: String,
    pub fix: Option<String>,
}

#[derive(Default, Debug)]
pub struct Diags {
    list: RefCell<Vec<Diag>>,
}

impl Diags {
    pub fn new() -> Diags {
        Diags::default()
    }

    pub fn add(&self, level: Level, code: &'static str, id: Option<&str>, path: Option<&str>, message: String) {
        self.push(Diag { level, code, id: id.map(String::from), path: path.map(String::from), message, fix: None });
    }

    pub fn add_fix(
        &self,
        level: Level,
        code: &'static str,
        id: Option<&str>,
        path: Option<&str>,
        message: String,
        fix: String,
    ) {
        self.push(Diag {
            level,
            code,
            id: id.map(String::from),
            path: path.map(String::from),
            message,
            fix: Some(fix),
        });
    }

    pub fn push(&self, d: Diag) {
        self.list.borrow_mut().push(d);
    }

    pub fn extend(&self, ds: impl IntoIterator<Item = Diag>) {
        self.list.borrow_mut().extend(ds);
    }

    pub fn list(&self) -> Ref<'_, Vec<Diag>> {
        self.list.borrow()
    }

    pub fn to_vec(&self) -> Vec<Diag> {
        self.list.borrow().clone()
    }

    pub fn take(&self) -> Vec<Diag> {
        core::mem::take(&mut *self.list.borrow_mut())
    }

    pub fn set(&self, ds: Vec<Diag>) {
        *self.list.borrow_mut() = ds;
    }

    pub fn into_vec(self) -> Vec<Diag> {
        self.list.into_inner()
    }

    pub fn len(&self) -> usize {
        self.list.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.borrow().is_empty()
    }

    pub fn err_count(&self) -> usize {
        self.list.borrow().iter().filter(|d| d.level == Level::Error).count()
    }

    pub fn warn_count(&self) -> usize {
        self.list.borrow().iter().filter(|d| d.level == Level::Warning).count()
    }

    pub fn has_code(&self, code: &str) -> bool {
        self.list.borrow().iter().any(|d| d.code == code)
    }
}

pub fn diag_to_json(d: &Diag) -> Value {
    let mut m = vec![
        json::Member { key: "level".into(), value: Value::String(d.level.name().into()) },
        json::Member { key: "code".into(), value: Value::String(d.code.into()) },
    ];
    if let Some(x) = &d.id {
        m.push(json::Member { key: "id".into(), value: Value::String(x.clone()) });
    }
    if let Some(x) = &d.path {
        m.push(json::Member { key: "path".into(), value: Value::String(x.clone()) });
    }
    m.push(json::Member { key: "message".into(), value: Value::String(d.message.clone()) });
    if let Some(x) = &d.fix {
        m.push(json::Member { key: "fix".into(), value: Value::String(x.clone()) });
    }
    Value::Object(m)
}

// ---- prisms ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Body,
    Line,
    Batt,
    Ghost,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OutlineMode {
    #[default]
    Full,
    Top,
    None,
}

#[derive(Clone, Debug)]
pub struct Prism {
    pub comp: u32,
    pub part: String,
    pub instance: u32,
    pub material: String,
    pub loops: Vec<Vec<Pt>>,
    pub z0: f64,
    pub z1: f64,
    pub embedded: bool,
    pub kind: Kind,
    pub outline: OutlineMode,
    pub pen: Option<Pen>,
    pub role: Role,
    pub quads: Vec<[V2; 4]>,
    pub blocking: bool,
    pub ply_lines: Vec<[V2; 2]>,
    pub line_pts: Vec<Pt>,
    pub centerline: Vec<Pt>,
    pub sweep_r: f64,
    pub cmu_unit: bool,
    pub course: u32,
    pub ticks: bool,
    pub zhalf: f64,
    /// Offset of the prism's z center from the component's z center (members seen lengthwise: flanges, legs, walls).
    pub zoff: f64,
    pub dashed: bool,
    pub face_tie: bool,
    /// Seen lengthwise (steel in elevation or plan): a section cut through it draws as an elevation (outlines, no fill).
    pub lengthwise: bool,
    /// The cap at z0 / z1 is where a void split the member along z (SPEC 26.4), not a face: iso and mesh draw only the
    /// part of that cap's outline the touching slab of the same member does not continue.
    pub seam: [bool; 2],
    /// Where the CMU running bond starts along z when a void split the wall into slabs (None: at z0).
    pub bond_z0: Option<f64>,
    /// One leg of a bar bent in the horizontal (XZ) plane (rebar `plane: "xz"`): the checks, iso and mesh use the legs;
    /// section, plan and elevation views draw the bar from `path3` instead.
    pub xz_bar: bool,
    /// The first leg of an XZ bar carries its whole centreline in world [x, y, z] (bends flattened).
    pub path3: Vec<[f64; 3]>,
    /// Framing hardware mounted on a member (SPEC 27.2): where nearer members hide it, its outline draws dashed (section,
    /// elevation, iso) and a note still lands on it.
    pub hw_dash: bool,
    /// Nail or screw dots on a plate seen face-on (SPEC 27.2), drawn where the plate is visible.
    pub nails: Vec<V2>,
}

impl Default for Prism {
    fn default() -> Prism {
        Prism {
            comp: 0,
            part: String::new(),
            instance: 0,
            material: "generic".to_string(),
            loops: Vec::new(),
            z0: 0.0,
            z1: 0.0,
            embedded: false,
            kind: Kind::Body,
            outline: OutlineMode::Full,
            pen: None,
            role: Role::Generic,
            quads: Vec::new(),
            blocking: false,
            ply_lines: Vec::new(),
            line_pts: Vec::new(),
            centerline: Vec::new(),
            sweep_r: 0.0,
            cmu_unit: false,
            course: 0,
            ticks: false,
            zhalf: 0.0,
            zoff: 0.0,
            dashed: false,
            face_tie: false,
            lengthwise: false,
            seam: [false; 2],
            bond_z0: None,
            xz_bar: false,
            path3: Vec::new(),
            hw_dash: false,
            nails: Vec::new(),
        }
    }
}

impl Prism {
    pub fn transform(&self, xf: &Xf) -> Prism {
        let mut q = self.clone();
        q.loops = self.loops.iter().map(|l| xf.apply_loop(l)).collect();
        q.quads = self.quads.iter().map(|quad| quad.map(|c| xf.apply(c))).collect();
        q.ply_lines = self.ply_lines.iter().map(|s| [xf.apply(s[0]), xf.apply(s[1])]).collect();
        if !self.line_pts.is_empty() {
            q.line_pts = xf.apply_loop(&self.line_pts);
        }
        if !self.centerline.is_empty() {
            q.centerline = xf.apply_loop(&self.centerline);
        }
        for n in &mut q.nails {
            *n = xf.apply(*n);
        }
        for c in &mut q.path3 {
            let w = xf.apply(V2::init(c[0], c[1]));
            *c = [w.x, w.y, c[2]];
        }
        q
    }
}

#[derive(Clone, Debug)]
pub struct NamedAnchor {
    pub name: String,
    pub p: V2,
}

#[derive(Clone, Debug)]
pub struct Zone {
    pub name: String,
    pub loops: Vec<Vec<Pt>>,
    pub box_: geom::Box,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cover {
    pub bottom: f64,
    pub sides: f64,
    pub top: f64,
}

impl Default for Cover {
    fn default() -> Cover {
        Cover { bottom: 3.0, sides: 3.0, top: 1.5 }
    }
}

#[derive(Clone, Debug)]
pub struct PartCover {
    pub part: String,
    pub cover: Cover,
}

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub outline: Vec<Pt>,
    pub cover: Cover,
    pub part_cover: Vec<PartCover>,
    /// The document sets `cover` (false: the builder default applies).
    pub cover_set: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Built {
    pub prisms: Vec<Prism>,
    pub anchors: Vec<NamedAnchor>,
    pub zones: Vec<Zone>,
    pub box_: geom::Box,
    pub nat_z: Option<f64>,
    pub points_mode: bool,
    pub host: Option<HostInfo>,
    pub centers: Vec<V2>,
    pub info: String,
    pub bar_d: f64,
    /// Named anchors that sit off the component's z centre (bars in the XZ plane): `p.y` is the z offset; plan views use it.
    pub plan_z: Vec<NamedAnchor>,
    /// The prisms' `zoff` are absolute z (bars in the XZ plane): without an explicit `z` the component is centred on z = 0.
    pub abs_z: bool,
    /// Angle (radians, CCW) of the top surface that `slope: "@id"` follows (types with `has_pitch`).
    pub pitch: f64,
    /// Fasteners of a `nailing` component, in world coordinates (SPEC 27.4).
    pub nails: Vec<Nail>,
    /// Indices of the components a `nailing` joins: its own members never hide its nails.
    pub nail_stack: Vec<u32>,
    /// A face `nailing` whose members also work listed the other way round (the last member is thinner than the nail).
    pub nail_flip: bool,
    /// Without an explicit `z` the component is centred here (hardware placed on its host members, SPEC 27.2).
    pub nat_zc: Option<f64>,
    /// What the hardware checks need after every component is placed (SPEC 27.2).
    pub hw: HwInfo,
}

/// One nail or screw in world [x, y, z]: the head on the face it is driven through, the tip where the shank ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Nail {
    pub head: [f64; 3],
    pub tip: [f64; 3],
    pub dia: f64,
    pub screw: bool,
    /// Unit direction of the nail's row and the o.c. spacing along it (zero for a lone nail): a section counts a row it
    /// sees end-on as cut within half a spacing of the plane.
    pub row: [f64; 3],
    pub pitch: f64,
    /// The component the head is on: the nail shows only in views that draw it.
    pub on: u32,
    /// Outward unit normal (world) of the face the head is on.
    pub face: [f64; 3],
}

/// A post base's bearing: plate bottom `y`, plan extent x0..x1 × z0..z1, the post and its standoff.
#[derive(Clone, Debug)]
pub struct Bears {
    pub y: f64,
    pub plan: [f64; 4],
    pub post: Vec<String>,
    pub post_y0: f64,
    pub standoff: f64,
}

/// Hardware findings (SPEC 27.2) the builder makes and `hwcheck` reports once the scene is complete.
#[derive(Clone, Debug, Default)]
pub struct HwInfo {
    /// Findings the builder could make on its own (unknown model, hanger size, holdown rod).
    pub warns: Vec<Diag>,
    /// A post base: its plate must bear on a member.
    pub bears: Option<Bears>,
    /// A holdown: its rod, whose lower end must be anchored in concrete, masonry or steel.
    pub rod: Option<String>,
    /// A hanger: the carried and the carrying members; the gap between them is the hanger's (no W_NEAR_MISS).
    pub joins: [Vec<String>; 2],
}

pub const BOX_ANCHOR_NAMES: [&str; 9] = [
    "top_left",
    "top_center",
    "top_right",
    "middle_left",
    "center",
    "middle_right",
    "bottom_left",
    "bottom_center",
    "bottom_right",
];

pub fn box_anchor(b: geom::Box, name: &str) -> Option<V2> {
    let cx = (b.x0 + b.x1) / 2.0;
    let cy = (b.y0 + b.y1) / 2.0;
    Some(match name {
        "top_left" => V2::init(b.x0, b.y1),
        "top_center" => V2::init(cx, b.y1),
        "top_right" => V2::init(b.x1, b.y1),
        "middle_left" => V2::init(b.x0, cy),
        "center" => V2::init(cx, cy),
        "middle_right" => V2::init(b.x1, cy),
        "bottom_left" => V2::init(b.x0, b.y0),
        "bottom_center" => V2::init(cx, b.y0),
        "bottom_right" => V2::init(b.x1, b.y0),
        _ => return None,
    })
}

pub fn rect_loop(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Pt> {
    vec![Pt::new(x0, y0), Pt::new(x1, y0), Pt::new(x1, y1), Pt::new(x0, y1)]
}

pub fn circle_loop(cx: f64, cy: f64, r: f64) -> Vec<Pt> {
    vec![Pt { x: cx + r, y: cy, b: 1.0 }, Pt { x: cx - r, y: cy, b: 1.0 }]
}

pub fn one_loop(l: Vec<Pt>) -> Vec<Vec<Pt>> {
    vec![l]
}

pub fn offset_pair_or_diag(diags: &Diags, id: Option<&str>, path: &str, what: &str, v: &Value) -> Option<V2> {
    let fix =
        format!("write {what} as [x, y], two lengths in inches (numbers or strings such as \"1 1/2\"), e.g. [1.5, -2]");
    let Some(arr) = v.arr() else {
        diags.add_fix(
            Level::Error,
            "E_PARAM",
            id,
            Some(path),
            format!("{what} must be an array [x, y] (got {})", kind_or_text(v)),
            fix,
        );
        return None;
    };
    if arr.len() != 2 {
        diags.add_fix(
            Level::Error,
            "E_PARAM",
            id,
            Some(path),
            format!("{what} must have exactly 2 values [x, y] (got {})", arr.len()),
            fix,
        );
        return None;
    }
    let mut out = [0.0; 2];
    for (i, e) in arr.iter().enumerate() {
        if let Some(x) = units::parse_length(e) {
            out[i] = x
        } else {
            if let Some(x) = units::parse_length_any(e) {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    id,
                    Some(path),
                    format!(
                        "{what}[{i}] is out of range: a length must be within +-{} inches (got {}, which is {} inches)",
                        fmt_num(limits::MAX_COORD_IN),
                        kind_or_text(e),
                        num_text(x)
                    ),
                    fix,
                );
            } else {
                diags.add_fix(
                    Level::Error,
                    "E_PARAM",
                    id,
                    Some(path),
                    format!(
                        "{what}[{i}] must be a length (got {}); lengths are {}",
                        kind_or_text(e),
                        units::LENGTH_FORMS
                    ),
                    fix,
                );
            }
            return None;
        }
    }
    Some(V2::init(out[0], out[1]))
}

pub fn length_or_diag(
    diags: &Diags,
    id: Option<&str>,
    path: &str,
    v: Option<&Value>,
    what: &str,
    default: f64,
) -> Option<f64> {
    let Some(x) = v else { return Some(default) };
    if x.is_null() {
        return Some(default);
    }
    if let Some(n) = units::parse_length(x) {
        return Some(n);
    }
    let fix = "write it as a number of inches or a length string such as \"1 1/2\"".to_string();
    if let Some(n) = units::parse_length_any(x) {
        diags.add_fix(
            Level::Error,
            "E_PARAM",
            id,
            Some(path),
            format!(
                "{what} is out of range: a length must be within +-{} inches (got {}, which is {} inches)",
                fmt_num(limits::MAX_COORD_IN),
                kind_or_text(x),
                num_text(n)
            ),
            fix,
        );
    } else {
        diags.add_fix(
            Level::Error,
            "E_PARAM",
            id,
            Some(path),
            format!("{what} must be a length (got {}); lengths are {}", kind_or_text(x), units::LENGTH_FORMS),
            fix,
        );
    }
    None
}

// ---- param extraction -------------------------------------------------------------------------------

pub struct Params<'a> {
    pub diags: &'a Diags,
    pub node: &'a Value,
    pub id: &'a str,
    pub base: &'a str,
    pub ty: &'a str,
    pub ok: bool,
}

impl<'a> Params<'a> {
    pub fn new(diags: &'a Diags, node: &'a Value, id: &'a str, base: &'a str, ty: &'a str) -> Params<'a> {
        Params { diags, node, id, base, ty, ok: true }
    }

    pub fn fail(&mut self, key: &str, message: String) {
        self.fail_code("E_PARAM", key, message);
    }

    pub fn fail_code(&mut self, code: &'static str, key: &str, message: String) {
        self.ok = false;
        let path = format!("{}/{}/{}", self.base, self.id, key);
        self.diags.add(Level::Error, code, Some(self.id), Some(&path), message);
    }

    pub fn parse<T: crate::params::ParamSet>(&mut self) -> Option<T> {
        let out = T::parse_all(self);
        if self.ok { Some(out) } else { None }
    }

    pub fn parse_all<T: crate::params::ParamSet>(&mut self) -> T {
        T::parse_all(self)
    }

    pub fn raw(&self, key: &str) -> Option<&'a Value> {
        let node: &'a Value = self.node;
        node.get(key).filter(|v| !v.is_null())
    }

    pub fn has(&self, key: &str) -> bool {
        self.raw(key).is_some()
    }

    pub fn missing(&mut self, key: &str, hint: &str) {
        let sep = if hint.is_empty() { "" } else { ": " };
        let m = format!("param '{key}' is required for type {}{sep}{hint}", self.ty);
        self.fail(key, m);
    }

    /// Length in inches; `default == None` means required.
    pub fn len(&mut self, key: &str, default: Option<f64>, hint: &str) -> Option<f64> {
        let Some(v) = self.raw(key) else {
            if default.is_some() {
                return default;
            }
            self.missing(key, hint);
            return None;
        };
        if let Some(x) = units::parse_length(v) {
            return Some(x);
        }
        if let Some(x) = units::parse_length_any(v) {
            self.fail(
                key,
                format!(
                    "param '{key}' is out of range: a length must be within +-{} inches (got {}, which is {} inches)",
                    fmt_num(limits::MAX_COORD_IN),
                    kind_or_text(v),
                    num_text(x)
                ),
            );
            return None;
        }
        self.fail(key, format!("param '{key}' must be a length: {} (got {})", units::LENGTH_FORMS, kind_or_text(v)));
        None
    }

    pub fn len_pos(&mut self, key: &str, default: Option<f64>, hint: &str) -> Option<f64> {
        let x = self.len(key, default, hint)?;
        if x <= 0.0 {
            self.fail(key, format!("param '{key}' must be greater than 0 (got {})", kind_or_text(&Value::Number(x))));
            return None;
        }
        Some(x)
    }

    pub fn num(&mut self, key: &str, default: Option<f64>) -> Option<f64> {
        let Some(v) = self.raw(key) else {
            if default.is_some() {
                return default;
            }
            self.fail(key, format!("param '{key}' is required for type {}", self.ty));
            return None;
        };
        if let Value::Number(n) = v {
            return Some(*n);
        }
        self.fail(key, format!("param '{key}' must be a number (got {})", kind_or_text(v)));
        None
    }

    pub fn int(&mut self, key: &str, default: Option<i64>, min: i64, max: i64) -> Option<i64> {
        let x = self.num(key, default.map(|d| d as f64))?;
        if x != crate::math::round(x) || x < min as f64 || x > max as f64 {
            self.fail(
                key,
                format!(
                    "param '{key}' must be an integer from {min} to {max} (got {})",
                    kind_or_text(&Value::Number(x))
                ),
            );
            return None;
        }
        num::to_int::<i64>(x)
    }

    // Members of object-valued params (place.cover, array.count, recess.from_edge ...): absent or null gives the default,
    // anything else must have the right type and range, else E_PARAM at `<key>/<field>` and None is returned.

    pub fn field_len(&mut self, key: &str, obj: &Value, field: &str, default: f64) -> Option<f64> {
        let v = match obj.get(field) {
            None => return Some(default),
            Some(v) if v.is_null() => return Some(default),
            Some(v) => v,
        };
        if let Some(x) = units::parse_length(v) {
            return Some(x);
        }
        let path = format!("{key}/{field}");
        if let Some(x) = units::parse_length_any(v) {
            self.fail_code(
                "E_PARAM",
                &path,
                format!(
                    "{key}.{field} is out of range: a length must be within +-{} inches (got {}, which is {} inches)",
                    fmt_num(limits::MAX_COORD_IN),
                    kind_or_text(v),
                    num_text(x)
                ),
            );
        } else {
            self.fail_code(
                "E_PARAM",
                &path,
                format!("{key}.{field} must be a length: {} (got {})", units::LENGTH_FORMS, kind_or_text(v)),
            );
        }
        None
    }

    pub fn field_int(&mut self, key: &str, obj: &Value, field: &str, default: i64, min: i64, max: i64) -> Option<i64> {
        let v = match obj.get(field) {
            None => return Some(default),
            Some(v) if v.is_null() => return Some(default),
            Some(v) => v,
        };
        if let Value::Number(x) = v
            && *x == crate::math::round(*x)
            && let Some(n) = num::to_int::<i64>(*x)
            && n >= min
            && n <= max
        {
            return Some(n);
        }
        let path = format!("{key}/{field}");
        self.fail_code(
            "E_PARAM",
            &path,
            format!("{key}.{field} must be an integer from {min} to {max} (got {})", kind_or_text(v)),
        );
        None
    }

    /// A string member that must be one of `choices` (the first is the default).
    pub fn field_choice(
        &mut self,
        key: &str,
        obj: &Value,
        field: &str,
        choices: &[&'static str],
    ) -> Option<&'static str> {
        let first = choices.first().copied()?;
        let v = match obj.get(field) {
            None => return Some(first),
            Some(v) if v.is_null() => return Some(first),
            Some(v) => v,
        };
        if let Value::String(s) = v
            && let Some(c) = choices.iter().find(|c| **c == s.as_str())
        {
            return Some(c);
        }
        let path = format!("{key}/{field}");
        self.fail_code(
            "E_PARAM",
            &path,
            format!("{key}.{field} must be one of {} (got {})", join_quoted(choices), kind_or_text(v)),
        );
        None
    }

    /// An `[dx, dy]` pair of lengths (offset of a Ref) at param `key`; any other shape is E_PARAM, never "0".
    pub fn offset_pair(&mut self, key: &str, v: &Value) -> Option<V2> {
        let path = format!("{}/{}/{}", self.base, self.id, key);
        let r = offset_pair_or_diag(self.diags, Some(self.id), &path, "offset", v);
        if r.is_none() {
            self.ok = false;
        }
        r
    }

    pub fn boolean(&mut self, key: &str, default: bool) -> bool {
        let Some(v) = self.raw(key) else { return default };
        if let Value::Bool(b) = v {
            return *b;
        }
        self.fail(key, format!("param '{key}' must be true or false (got {})", kind_or_text(v)));
        default
    }

    pub fn str(&mut self, key: &str, default: Option<&'a str>) -> Option<&'a str> {
        let Some(v) = self.raw(key) else {
            if default.is_some() {
                return default;
            }
            self.fail(key, format!("param '{key}' is required for type {}", self.ty));
            return None;
        };
        if let Value::String(s) = v {
            return Some(s);
        }
        self.fail(key, format!("param '{key}' must be a string (got {})", kind_or_text(v)));
        None
    }

    /// One of `allowed`.
    pub fn choice(
        &mut self,
        key: &str,
        default: Option<&'static str>,
        allowed: &[&'static str],
    ) -> Option<&'static str> {
        let s = self.str(key, default)?;
        if let Some(al) = allowed.iter().find(|al| **al == s) {
            return Some(al);
        }
        self.fail(key, format!("param '{key}' must be one of {} (got \"{s}\")", join_quoted(allowed)));
        None
    }
}

pub fn join_quoted(items: &[&str]) -> String {
    let mut out = String::new();
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(it);
        out.push('"');
    }
    out
}

/// The Kerf number text (`json::fmt_number`) as an owned string.
pub fn fmt_num(x: f64) -> String {
    let mut b = [0u8; 40];
    json::fmt_number(&mut b, x).to_string()
}

/// A number for a message: plain up to 1e9 and down to 1e-4, scientific beyond (so 1e300 does not print 300 digits).
pub fn num_text(x: f64) -> String {
    if !x.is_finite() {
        return "a non-finite number".to_string();
    }
    let m = x.abs();
    if m != 0.0 && !(1e-4..1e9).contains(&m) { format!("{x:e}") } else { fmt_num(x) }
}

pub fn kind_or_text(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Number(n) => fmt_num(*n),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.kind_name().to_string(),
    }
}

// ---- text helpers ------------------------------------------------------------------------------------

pub fn edit_distance(x: &str, y: &str) -> usize {
    let (x, y) = (x.as_bytes(), y.as_bytes());
    let n = y.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for (i, &cx) in x.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cy) in y.iter().enumerate() {
            let sub = prev[j] + usize::from(cx != cy);
            cur[j + 1] = sub.min((prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev.copy_from_slice(&cur);
    }
    prev[n]
}

/// Closest of `names` to `x` (None when nothing is reasonably close or `names` is empty).
pub fn nearest<'n, S: AsRef<str>>(x: &str, names: &'n [S]) -> Option<&'n str> {
    let mut best: Option<&str> = None;
    let mut bd = usize::MAX;
    for n in names {
        let d = edit_distance(x, n.as_ref());
        if d < bd {
            bd = d;
            best = Some(n.as_ref());
        }
    }
    if best.is_some() && bd <= 3.max(x.len() / 2) { best } else { None }
}
