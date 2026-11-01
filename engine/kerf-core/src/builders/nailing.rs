//! nailing (SPEC 27.4): nails and screws placed from the members they join. Its parameters (`Params`, the single source for the
//! parser and the catalog), the fastener size table, the note text form and the builder.
//!
//! Every member instance is a box in the frame of the first member (its local profile box times its z range): the builder
//! finds the joint face between the boxes, lays the nails out on it and stores them in world coordinates (`Built.nails`).

use super::common::{Ctx, fmt_ft_in, fmt_num, parse_actual};
use crate::geom::{Box, V2, Xf};
use crate::json::Value;
use crate::math;
use crate::model::{self, Built, Level, Nail};
use crate::params::{ParamEnum, param_enum, params};
use crate::scene::{self, Comp, State};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Pattern { Face = "face", Toenail = "toenail", Edge = "edge", Field = "field" } }
param_enum! { pub enum Fastener { Nail = "nail", Screw = "screw" } }
param_enum! { pub enum Axis { X = "x", Y = "y", Z = "z" } }

params! {
    pub struct Params {
        members: OptRaw => { def: "required", desc: "component ids the fasteners join, in driving order: the first is the member the heads are on (face, toenail: the piece nailed; edge, field: the sheathing panel; several panels side by side: list them all first, e.g. [\"sh1\", \"sh2\", \"sh3\", \"joists\", \"rim\"]), the rest are what the points go into (`id#k` for one array instance; every instance of an array otherwise, so an arrayed panel is nailed sheet by sheet). Placement comes from these members: `at`, `z`, `array`, `rotate` are not accepted" },
        pattern: Enum(Pattern) = Pattern::Face => { desc: "face: nails square to the joint face, through the first member into the next (plate splices, sill and plate nailing, built-up plies); toenail: angled from a side face of the first member into the second (blocking, studs, rafters to plates); edge: rows at `edge_distance` along the panel edges, kept where framing lies behind; field: rows along the intermediate framing behind the panel" },
        size: Str => { desc: "nail: penny size \"16d\" (common; \"16d box\", \"16d sinker\"), or shank x length \"0.131x3\"; screw: \"SDS25300\", \"#10x3\", \"1/4x3 1/2\" with fastener screw. Drawn length and diameter come from it" },
        fastener: Enum(Fastener) = Fastener::Nail => { desc: "nail | screw (SDS and #N sizes are screws)" },
        count: OptInt(u16, 1, 500) => { desc: "face, toenail: nails per joint (total over all rows and faces). With `spacing` too: that many at that spacing, centered on the joint" },
        spacing: OptLenPos => { def: "edge 6, field 12", desc: "o.c. spacing along a row (with `stagger`: within each row, the rows offset by spacing/rows)" },
        rows: Int(u8, 1, 4) = 1 => { desc: "face: rows across the joint, evenly spaced over its width" },
        stagger: Bool = true => { desc: "rows > 1: alternate nails between rows (false: the rows line up)" },
        along: OptEnum(Axis) => { desc: "face: world axis the rows run along (default: the joint's longer side)" },
        range: OptRaw => { desc: "face, toenail: [from, to] limits the rows along their axis: Refs or [x, y] points (projected on the row), or z values for a row along z" },
        edges: OptRaw => { def: "all four", desc: "edge: panel edges to nail, by direction: left right top bottom front back" },
        faces: OptRaw => { desc: "toenail: side faces of the first member the nails start on (left right top bottom front back); default both faces whose normal lies in the drawing plane" },
        edge_distance: OptLenPos => { def: "edge, field 3/8; face 10D (10 x shank dia.)", desc: "distance from the panel edge (edge, field) or from the ends of the joint (face) to the first nail" },
        angle: Num = 30.0 => { desc: "toenail: degrees between the nail and the member (30 typical, started 1/3 of the nail length from the end)" },
        through: Bool = false => { desc: "the points may come out of the far face (plate splice face nails, clinched nails): no W_NAIL_LENGTH for that" },
        blocked: Bool = true => { desc: "edge: false for an unblocked diaphragm or wall: edge nails go only where framing lies behind the edge, and edges without framing are not W_NAIL_EDGE" },
        text: Str = "" => { def: "generated", desc: "note text form override; the default is generated from the params (\"(8) 16d NAILS STAGGERED\", \"8d @ 6\\\" O.C. EDGE NAILING\") and a note targeting this component without `text` uses it" },
    }
}

/// A fastener size: shank diameter and length (inches), whether it is a screw, and the text the notes print.
#[derive(Clone, Debug, PartialEq)]
pub struct Size {
    pub dia: f64,
    pub len: f64,
    pub screw: bool,
    pub text: String,
}

const COMMON: [(u32, f64, f64); 11] = [
    (4, 0.099, 1.5),
    (6, 0.113, 2.0),
    (8, 0.131, 2.5),
    (10, 0.148, 3.0),
    (12, 0.148, 3.25),
    (16, 0.162, 3.5),
    (20, 0.192, 4.0),
    (30, 0.207, 4.5),
    (40, 0.225, 5.0),
    (50, 0.244, 5.5),
    (60, 0.263, 6.0),
];
const BOX: [(u32, f64, f64); 6] =
    [(6, 0.099, 2.0), (8, 0.113, 2.5), (10, 0.128, 3.0), (12, 0.128, 3.25), (16, 0.135, 3.5), (20, 0.148, 4.0)];
const SINKER: [(u32, f64, f64); 6] =
    [(6, 0.092, 1.875), (8, 0.113, 2.375), (10, 0.12, 2.875), (12, 0.135, 3.125), (16, 0.148, 3.25), (20, 0.177, 3.75)];
const SCREW_GAUGE: [(u32, f64); 5] = [(6, 0.138), (8, 0.164), (10, 0.19), (12, 0.216), (14, 0.242)];

pub const SIZE_FORMS: &str = "penny sizes 4d 6d 8d 10d 12d 16d 20d 30d 40d 50d 60d (common; add \" box\" or \" sinker\"), shank x length \"0.131x3\", or screws \"SDS25300\" (1/4\" SDS, length code 112 200 212 300 312 412 500 600 800), \"#10x3\"";

/// Parse a fastener size. `screw` is the `fastener` param (SDS and #N sizes are screws regardless).
pub fn parse_size(s: &str, screw: bool) -> Option<Size> {
    let t = s.trim_matches(' ');
    let lower = t.to_ascii_lowercase();
    let text = t.to_string();
    if let Some(code) = lower.strip_prefix("sds25") {
        let b = code.as_bytes();
        if b.len() == 3 && b.iter().all(u8::is_ascii_digit) {
            let whole = f64::from(b[0] - b'0');
            let frac = match &code[1..] {
                "00" => 0.0,
                "12" => 0.5,
                _ => return None,
            };
            let len = whole + frac;
            return (len >= 1.0).then(|| Size { dia: 0.25, len, screw: true, text: t.to_ascii_uppercase() });
        }
        return None;
    }
    if let Some(rest) = lower.strip_prefix('#') {
        let xi = rest.find(['x', '*'])?;
        let g: u32 = rest[..xi].trim_matches(' ').parse().ok()?;
        let dia = SCREW_GAUGE.iter().find(|e| e.0 == g)?.1;
        let len = crate::units::parse_length_str(rest[xi + 1..].trim_matches(' '))?;
        return (len > 0.0 && len <= 24.0).then_some(Size { dia, len, screw: true, text });
    }
    let (num, kind) = match lower.split_once(' ') {
        Some((a, b)) => (a, b.trim_matches(' ')),
        None => (lower.as_str(), "common"),
    };
    if let Some(n) = num.strip_suffix('d')
        && let Ok(n) = n.parse::<u32>()
    {
        let table: &[(u32, f64, f64)] = match kind {
            "common" | "" => &COMMON,
            "box" => &BOX,
            "sinker" => &SINKER,
            _ => return None,
        };
        let e = table.iter().find(|e| e.0 == n)?;
        return Some(Size { dia: e.1, len: e.2, screw, text });
    }
    let a = parse_actual(t)?;
    (a.t < 1.0 && a.d <= 24.0 && a.d > a.t).then_some(Size { dia: a.t, len: a.d, screw, text })
}

/// Whole inches with a sixteenths fraction: `6"`, `12"`, `2 1/2"` (feet-inches from 10' up).
pub fn inch_text(x: f64) -> String {
    if x >= 120.0 {
        return fmt_ft_in(x);
    }
    let whole = math::floor(x + 1.0 / 32.0);
    let frac = x - whole;
    if whole < 1.0 {
        return fmt_ft_in(x);
    }
    if frac.abs() < 1.0 / 32.0 {
        return format!("{}\"", fmt_num(whole));
    }
    let f = fmt_ft_in(frac);
    format!("{} {f}", fmt_num(whole))
}

/// Inputs of the text form, from the parsed params or straight from a document node.
pub struct TextIn<'a> {
    pub pattern: Pattern,
    pub size: &'a str,
    pub screw: bool,
    pub count: Option<u32>,
    pub spacing: Option<f64>,
    pub rows: u32,
    pub stagger: bool,
}

/// The note text form: `(8) 16d NAILS STAGGERED`, `16d NAILS @ 12" O.C.`, `(2) 16d TOENAILS`, `8d @ 6" O.C. EDGE NAILING`.
pub fn text_form(t: &TextIn) -> String {
    let size = t.size.trim_matches(' ');
    let spacing = t.spacing.or(match t.pattern {
        Pattern::Edge => Some(6.0),
        Pattern::Field => Some(12.0),
        _ => None,
    });
    let oc = |s: f64| format!("@ {} O.C.", inch_text(s));
    let mut out = match t.pattern {
        Pattern::Edge | Pattern::Field => {
            let what = match (t.pattern, t.screw) {
                (Pattern::Edge, false) => "EDGE NAILING",
                (Pattern::Edge, true) => "EDGE SCREWS",
                (_, false) => "FIELD NAILING",
                (_, true) => "FIELD SCREWS",
            };
            format!("{size} {} {what}", oc(spacing.unwrap_or(0.0)))
        }
        Pattern::Face | Pattern::Toenail => {
            let noun = match (t.pattern, t.screw) {
                (Pattern::Toenail, false) => "TOENAILS",
                (Pattern::Toenail, true) => "TOE SCREWS",
                (_, false) => "NAILS",
                (_, true) => "SCREWS",
            };
            match (t.count, spacing) {
                (Some(n), Some(s)) => format!("({n}) {size} {noun} {}", oc(s)),
                (Some(n), None) => format!("({n}) {size} {noun}"),
                (None, Some(s)) => format!("{size} {noun} {}", oc(s)),
                (None, None) => format!("{size} {noun}"),
            }
        }
    };
    if t.pattern == Pattern::Face && t.rows > 1 {
        if t.stagger {
            out.push_str(" STAGGERED");
        } else {
            out.push_str(&format!(" IN {} ROWS", t.rows));
        }
    }
    out
}

/// The text form of a `nailing` document node (no diagnostics; bad values fall back to defaults). Coverage and notes use it.
pub fn node_text(node: &Value) -> String {
    let s = |k: &str| node.get(k).and_then(Value::str).unwrap_or("");
    let own = s("text");
    if !own.trim_matches(' ').is_empty() {
        return own.to_string();
    }
    let pattern = Pattern::from_tag(s("pattern")).unwrap_or(Pattern::Face);
    let size = s("size");
    let screw = s("fastener") == "screw" || parse_size(size, false).is_some_and(|z| z.screw);
    let count = node.get("count").and_then(Value::num).filter(|n| *n >= 1.0).and_then(num_u32);
    let spacing = node.get("spacing").and_then(crate::units::parse_length).filter(|x| *x > 0.0);
    let rows = node.get("rows").and_then(Value::num).and_then(num_u32).unwrap_or(1);
    let stagger = !matches!(node.get("stagger"), Some(Value::Bool(false)));
    text_form(&TextIn { pattern, size, screw, count, spacing, rows, stagger })
}

fn num_u32(x: f64) -> Option<u32> {
    crate::num::to_int::<u32>(math::round(x))
}

// ---- member boxes ------------------------------------------------------------------------------------

const TOL: f64 = 0.01;
/// Framing whose centerline is this close to a panel edge carries edge nailing (a built-up chord, a doubled stud at a
/// joint), not field nailing.
const EDGE_ZONE: f64 = 3.0;
/// Field nails stop this far short of an edge row (the 2" minimum spacing of panel nails).
const FIELD_CLEAR: f64 = 2.0;

/// One member instance: its local box and the transforms between its local frame and the world.
#[derive(Clone, Copy)]
struct Member {
    comp: u32,
    inst: usize,
    xf: Xf,
    inv: Xf,
    lo: [f64; 3],
    hi: [f64; 3],
}

fn inverse(x: &Xf) -> Xf {
    let det = x.det();
    let (a, b, c, d) = (x.d / det, -x.b / det, -x.c / det, x.a / det);
    Xf { a, b, c, d, tx: -(a * x.tx + c * x.ty), ty: -(b * x.tx + d * x.ty) }
}

fn members_of(c: &Comp, only: Option<usize>) -> Vec<Member> {
    let b = c.built.box_;
    (0..c.xfs.len())
        .filter(|k| only.is_none_or(|o| o == *k))
        .filter_map(|k| {
            let xf = c.xfs[k];
            let z = *c.zs.get(k)?;
            (xf.det().abs() > 1e-12).then(|| Member {
                comp: c.index,
                inst: k,
                xf,
                inv: inverse(&xf),
                lo: [b.x0, b.y0, z[0]],
                hi: [b.x1, b.y1, z[1]],
            })
        })
        .collect()
}

fn member_name(scene: &scene::Scene, m: &Member) -> String {
    let Some(c) = scene.comps.get(m.comp as usize) else { return String::new() };
    if c.xfs.len() > 1 { format!("{}#{}", c.id, m.inst) } else { c.id.clone() }
}

/// `b`'s box in `a`'s frame (the bounding box of its corners).
fn box_in(a: &Member, b: &Member) -> ([f64; 3], [f64; 3]) {
    let mut bx = Box::default();
    for (x, y) in [(b.lo[0], b.lo[1]), (b.hi[0], b.lo[1]), (b.hi[0], b.hi[1]), (b.lo[0], b.hi[1])] {
        let w = b.xf.apply(V2::init(x, y));
        let l = a.inv.apply(w);
        bx.add_point(l.x, l.y);
    }
    ([bx.x0, bx.y0, b.lo[2]], [bx.x1, bx.y1, b.hi[2]])
}

fn to_world(a: &Member, p: [f64; 3]) -> [f64; 3] {
    let w = a.xf.apply(V2::init(p[0], p[1]));
    [w.x, w.y, p[2]]
}

fn dir_world(a: &Member, d: [f64; 3]) -> [f64; 3] {
    let w = a.xf.apply_dir(V2::init(d[0], d[1]));
    let n = math::sqrt(w.x * w.x + w.y * w.y + d[2] * d[2]);
    if n < 1e-12 { [0.0; 3] } else { [w.x / n, w.y / n, d[2] / n] }
}

fn unit(axis: usize, sign: f64) -> [f64; 3] {
    let mut u = [0.0; 3];
    u[axis] = sign;
    u
}

/// The name of a's local axis direction in the world: left/right/top/bottom by its dominant in-plane component, front/back for z.
fn face_name(a: &Member, axis: usize, sign: f64) -> &'static str {
    if axis == 2 {
        return if sign > 0.0 { "front" } else { "back" };
    }
    let d = a.xf.apply_dir(V2::init(if axis == 0 { sign } else { 0.0 }, if axis == 1 { sign } else { 0.0 }));
    if d.x.abs() >= d.y.abs() {
        if d.x > 0.0 { "right" } else { "left" }
    } else if d.y > 0.0 {
        "top"
    } else {
        "bottom"
    }
}

const FACE_NAMES: [&str; 6] = ["left", "right", "top", "bottom", "front", "back"];

fn axis_of_name(a: &Member, name: &str) -> Option<(usize, f64)> {
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            if face_name(a, axis, sign) == name {
                return Some((axis, sign));
            }
        }
    }
    None
}

/// A's local axis most parallel to a world axis.
fn local_axis(a: &Member, w: Axis) -> usize {
    match w {
        Axis::Z => 2,
        Axis::X | Axis::Y => {
            let comp = |v: V2| if w == Axis::X { v.x.abs() } else { v.y.abs() };
            if comp(a.xf.apply_dir(V2::init(1.0, 0.0))) >= comp(a.xf.apply_dir(V2::init(0.0, 1.0))) { 0 } else { 1 }
        }
    }
}

/// Where two boxes (in one frame) touch face to face: the axis, the side of `b` (+1: b beyond a's high face) and the overlap
/// of the faces in the other two axes. The largest touching face wins.
#[derive(Clone, Copy)]
struct Contact {
    axis: usize,
    side: f64,
    ov: [[f64; 2]; 3],
}

fn contact(alo: [f64; 3], ahi: [f64; 3], blo: [f64; 3], bhi: [f64; 3]) -> Option<Contact> {
    let mut best: Option<(f64, Contact)> = None;
    for axis in 0..3 {
        for side in [1.0, -1.0] {
            let touch = if side > 0.0 { (ahi[axis] - blo[axis]).abs() } else { (alo[axis] - bhi[axis]).abs() };
            if touch > TOL {
                continue;
            }
            let mut ov = [[0.0; 2]; 3];
            let mut area = 1.0;
            let mut ok = true;
            for o in 0..3 {
                ov[o] = [alo[o].max(blo[o]), ahi[o].min(bhi[o])];
                if o != axis {
                    let w = ov[o][1] - ov[o][0];
                    if w <= TOL {
                        ok = false;
                    }
                    area *= w;
                }
            }
            if ok && best.is_none_or(|(b, _)| area > b + 1e-9) {
                best = Some((area, Contact { axis, side, ov }));
            }
        }
    }
    best.map(|(_, c)| c)
}

/// How two boxes miss each other: the gap along each separated axis, or that they overlap.
fn miss_text(alo: [f64; 3], ahi: [f64; 3], blo: [f64; 3], bhi: [f64; 3]) -> String {
    let names = ["x", "y", "z"];
    let mut gaps = Vec::new();
    for o in 0..3 {
        let g = (blo[o] - ahi[o]).max(alo[o] - bhi[o]);
        if g > TOL {
            gaps.push(format!("{} apart along {}", fmt_ft_in(g), names[o]));
        }
    }
    if gaps.is_empty() { "their boxes overlap instead of meeting face to face".to_string() } else { gaps.join(", ") }
}

// ---- layout along a row ----------------------------------------------------------------------------

/// Positions from `a` to `b` (both included) at a step of at most `s`.
fn fill_span(a: f64, b: f64, s: f64) -> Vec<f64> {
    let len = b - a;
    if len <= 1e-9 || s <= 0.0 {
        return vec![(a + b) / 2.0];
    }
    let n = (math::ceil(len / s - 1e-9) as usize).clamp(1, 5000);
    (0..=n).map(|k| a + len * k as f64 / n as f64).collect()
}

/// `n` positions at step `s` centered on `[a, b]`.
fn centered(a: f64, b: f64, n: usize, s: f64) -> Vec<f64> {
    let mid = (a + b) / 2.0;
    let start = mid - (n.saturating_sub(1)) as f64 * s / 2.0;
    (0..n).map(|k| start + k as f64 * s).collect()
}

struct Warn {
    code: &'static str,
    msg: String,
    fix: String,
}

struct Out {
    nails: Vec<Nail>,
    warns: Vec<Warn>,
    /// Face nailing: the last member is thinner than the nail, so the members listed the other way round work too.
    flip: bool,
}

impl Out {
    fn warn(&mut self, code: &'static str, msg: String, fix: String) {
        if self.warns.iter().any(|w| w.code == code && w.msg == msg) {
            return;
        }
        self.warns.push(Warn { code, msg, fix });
    }

    fn push(&mut self, n: Nail) {
        if self.nails.len() > crate::limits::MAX_POINTS {
            return;
        }
        let dup = self
            .nails
            .iter()
            .any(|m| (0..3).all(|k| (m.head[k] - n.head[k]).abs() < 1e-6 && (m.tip[k] - n.tip[k]).abs() < 1e-6));
        if !dup {
            self.nails.push(n);
        }
    }
}

struct Job<'a> {
    id: &'a str,
    size: &'a Size,
    cp: &'a Params,
    through: bool,
    scene: &'a scene::Scene<'a>,
    /// Member components in document order; each entry is every instance (or the one named with `#k`).
    groups: Vec<Vec<Member>>,
    /// Edge and field: how many leading groups are the sheets the heads are on (the rest are the framing).
    sheets: usize,
    /// The range entries as written (points resolved to world XY; numbers are z).
    range_pts: Option<[RangeEnd; 2]>,
    /// The nailing's `views` (its scenario): fixes only name components drawn in one of them.
    views: Option<&'a Value>,
}

#[derive(Clone, Copy)]
enum RangeEnd {
    Pt(V2),
    Z(f64),
}

fn op_update(id: &str, key: &str, value: &str) -> String {
    format!("{{\"op\":\"update\",\"path\":\"components/{id}\",\"value\":{{\"{key}\":{value}}}}}")
}

fn quoted_list(items: &[String]) -> String {
    let q: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", q.join(","))
}

impl Job<'_> {
    /// `c` is drawn in a view the nailing is drawn in (either without `views` counts as every view).
    fn shares_view(&self, c: &Comp) -> bool {
        let names = |v: &Value| -> Vec<String> {
            v.arr().map_or(Vec::new(), |a| a.iter().filter_map(Value::str).map(String::from).collect())
        };
        match (self.views.map(names), c.node.get("views").filter(|v| !v.is_null()).map(names)) {
            (Some(mine), Some(theirs)) => mine.iter().any(|v| theirs.contains(v)),
            _ => true,
        }
    }

    fn members_json(&self) -> Vec<String> {
        match self.cp.members.as_ref().and_then(Value::arr) {
            Some(a) => a.iter().filter_map(Value::str).map(String::from).collect(),
            None => Vec::new(),
        }
    }

    /// Components (other than the nailing's members) whose boxes touch `a` face to face, for a fix.
    /// Components not in the member list that lie against panel `a`'s face (thin axis `k`) behind any of `pts`.
    fn backing(&self, a: &Member, k: usize, pts: &[[f64; 3]]) -> Vec<String> {
        let listed: Vec<u32> = self.groups.iter().flat_map(|g| g.iter().map(|m| m.comp)).collect();
        let mut out: Vec<String> = Vec::new();
        for c in &self.scene.comps {
            if c.state != State::Ok
                || c.index == a.comp
                || listed.contains(&c.index)
                || c.built.prisms.is_empty()
                || !self.shares_view(c)
            {
                continue;
            }
            if self.scene.comps.get(a.comp as usize).is_some_and(|ac| !self.scene.together(ac, c)) {
                continue;
            }
            if c.ty.ty == crate::catalog::Type::Nailing || c.embedded || c.ty.traits.hardware {
                continue;
            }
            let backs = members_of(c, None).iter().any(|b| {
                let (lo, hi) = box_in(a, b);
                let touches = (a.lo[k] - hi[k]).abs() <= TOL || (a.hi[k] - lo[k]).abs() <= TOL;
                touches
                    && pts
                        .iter()
                        .any(|p| (0..3).filter(|&o| o != k).all(|o| p[o] > lo[o] + 1e-6 && p[o] < hi[o] - 1e-6))
            });
            if backs && !out.contains(&c.id) {
                out.push(c.id.clone());
            }
        }
        out
    }

    /// A component that is not a member lies against the stack's far face (`far` along `a`'s axis `k`, beyond it on
    /// `side`) at `at`: nails driven from that side would start inside the framing.
    fn covered(&self, a: &Member, k: usize, side: f64, far: f64, at: [f64; 3]) -> bool {
        let listed: Vec<u32> = self.groups.iter().flat_map(|g| g.iter().map(|m| m.comp)).collect();
        self.scene.comps.iter().any(|c| {
            if c.state != State::Ok || listed.contains(&c.index) || c.built.prisms.is_empty() || !self.shares_view(c) {
                return false;
            }
            if c.ty.ty == crate::catalog::Type::Nailing || c.embedded || c.ty.traits.hardware {
                return false;
            }
            members_of(c, None).iter().any(|m| {
                let (lo, hi) = box_in(a, m);
                let near = if side > 0.0 { lo[k] } else { hi[k] };
                (near - far).abs() <= TOL
                    && (0..3).filter(|&o| o != k).all(|o| at[o] > lo[o] + 1e-6 && at[o] < hi[o] - 1e-6)
            })
        })
    }

    fn touching(&self, a: &Member) -> Vec<String> {
        let listed: Vec<u32> = self.groups.iter().flat_map(|g| g.iter().map(|m| m.comp)).collect();
        let mut out: Vec<String> = Vec::new();
        for c in &self.scene.comps {
            if c.state != State::Ok
                || c.index == a.comp
                || listed.contains(&c.index)
                || c.built.prisms.is_empty()
                || !self.shares_view(c)
            {
                continue;
            }
            if self.scene.comps.get(a.comp as usize).is_some_and(|ac| !self.scene.together(ac, c)) {
                continue;
            }
            if c.ty.ty == crate::catalog::Type::Nailing || c.embedded || c.ty.traits.hardware {
                continue;
            }
            if members_of(c, None).iter().any(|b| {
                let (lo, hi) = box_in(a, b);
                contact(a.lo, a.hi, lo, hi).is_some()
            }) && !out.contains(&c.id)
            {
                out.push(c.id.clone());
            }
        }
        out
    }

    fn miss(&self, out: &mut Out, a: &Member, b: &Member) {
        let (lo, hi) = box_in(a, b);
        let an = member_name(self.scene, a);
        let bn = member_name(self.scene, b);
        let how = miss_text(a.lo, a.hi, lo, hi);
        let near = self.touching(a);
        let fix = if near.is_empty() {
            format!(
                "move '{bn}' so it bears on a face of '{an}' (nails join members that touch face to face), or list the members the fasteners actually join"
            )
        } else {
            let mut m = self.members_json();
            if m.len() >= 2 {
                m[1] = near[0].clone();
            }
            format!(
                "members touching '{an}': {}. E.g. {}",
                scene::join_ids(&near),
                op_update(self.id, "members", &quoted_list(&m))
            )
        };
        out.warn(
            "W_NAIL_MEMBERS",
            format!("'{}': '{an}' and '{bn}' do not touch ({how}), so no fasteners are drawn between them", self.id),
            fix,
        );
    }

    fn range_on(&self, a: &Member, axis: usize) -> Option<[f64; 2]> {
        let ends = self.range_pts?;
        let mut v = [0.0; 2];
        for (k, e) in ends.iter().enumerate() {
            v[k] = match (e, axis) {
                (RangeEnd::Z(z), 2) => *z,
                (RangeEnd::Pt(p), 0 | 1) => {
                    let l = a.inv.apply(*p);
                    if axis == 0 { l.x } else { l.y }
                }
                _ => return None,
            };
        }
        Some([v[0].min(v[1]), v[0].max(v[1])])
    }

    fn nail(&self, a: &Member, head: [f64; 3], dir: [f64; 3], row: [f64; 3], pitch: f64) -> Nail {
        let len = self.size.len;
        let tip = [head[0] + dir[0] * len, head[1] + dir[1] * len, head[2] + dir[2] * len];
        Nail {
            head: to_world(a, head),
            tip: to_world(a, tip),
            dia: self.size.dia,
            screw: self.size.screw,
            row: if pitch > 0.0 { dir_world(a, row) } else { [0.0; 3] },
            pitch,
            on: a.comp,
            face: dir_world(a, [-dir[0], -dir[1], -dir[2]]),
        }
    }

    /// Length check for nails square to a stack: `held` names the members the points hold in, `head` = thickness the
    /// nail passes before them, `before` = thickness ahead of the last member, `total` = the stack.
    fn check_length(&self, out: &mut Out, held: &str, last: &str, head: f64, before: f64, total: f64) {
        let l = self.size.len;
        let d = self.size.dia;
        let id = self.id;
        if l <= before + TOL {
            let fit = longer_size(self.size, before + 6.0 * d);
            out.warn(
                "W_NAIL_LENGTH",
                format!(
                    "'{id}': {} ({}) stops {} into the {} of members ahead of '{last}' and never reaches it",
                    self.size.text,
                    fmt_ft_in(l),
                    fmt_ft_in(l),
                    fmt_ft_in(before)
                ),
                match fit {
                    Some(f) => op_update(id, "size", &format!("\"{f}\"")),
                    None => "use a screw or bolt long enough for the stack".to_string(),
                },
            );
        } else if l - head < 6.0 * d - 1e-9 {
            let fit = longer_size(self.size, head + 6.0 * d);
            out.warn(
                "W_NAIL_LENGTH",
                format!(
                    "'{id}': {} ({}) penetrates {held} only {}, under 6 x the {} shank (6D = {})",
                    self.size.text,
                    fmt_ft_in(l),
                    fmt_ft_in(l - head),
                    fmt_num(d),
                    fmt_ft_in(6.0 * d)
                ),
                match fit {
                    Some(f) => op_update(id, "size", &format!("\"{f}\"")),
                    None => "use a longer fastener".to_string(),
                },
            );
        } else if l > total + TOL && !self.through {
            let fit = shorter_size(self.size, before + 6.0 * d, total);
            let mut fix = String::new();
            if let Some(f) = fit {
                fix.push_str(&format!("{} stays inside: {}; or ", f, op_update(id, "size", &format!("\"{f}\""))));
            }
            fix.push_str(&format!(
                "where the points are meant to come through (plate splice face nails, clinched nails): {}",
                op_update(id, "through", "true")
            ));
            out.warn(
                "W_NAIL_LENGTH",
                format!(
                    "'{id}': {} ({}) is longer than the {} stack it is driven through; the points come out {} past the far face of '{last}'",
                    self.size.text,
                    fmt_ft_in(l),
                    fmt_ft_in(total),
                    fmt_ft_in(l - total)
                ),
                fix,
            );
        }
    }

    fn check_spacing(&self, out: &mut Out, row_spacing: f64, panel: bool) {
        let d = self.size.dia;
        let min = if panel { 2.0f64.max(10.0 * d) } else { 10.0 * d };
        if row_spacing + 1e-9 >= min || row_spacing <= 0.0 {
            return;
        }
        let what = if panel {
            "the 2\" o.c. (and 10D) minimum for panel nailing"
        } else {
            "the 10D minimum between nails of a row"
        };
        let fix = if self.cp.count.is_some() {
            format!(
                "fewer nails, more rows or a longer joint (e.g. {} or {})",
                op_update(self.id, "rows", &format!("{}", (self.cp.rows + 1).min(4))),
                op_update(self.id, "spacing", &fmt_num(round_up_quarter(min)))
            )
        } else {
            op_update(self.id, "spacing", &fmt_num(round_up_quarter(min)))
        };
        out.warn(
            "W_NAIL_SPACING",
            format!(
                "'{}': nails of one row are {} apart, under {what} ({} for {})",
                self.id,
                fmt_ft_in(row_spacing),
                fmt_ft_in(min),
                self.size.text
            ),
            fix,
        );
    }

    fn check_edge(&self, out: &mut Out, e: f64, what: &str) {
        let min = 0.375f64.max(if what == "across" { 2.5 * self.size.dia } else { 0.0 });
        if e + 1e-9 >= min {
            return;
        }
        let (msg, fix) = if what == "across" {
            (
                format!(
                    "'{}': the outer rows are {} from the edges of the joint, under the {} minimum edge distance",
                    self.id,
                    fmt_ft_in(e),
                    fmt_ft_in(min)
                ),
                op_update(self.id, "rows", &format!("{}", self.cp.rows.saturating_sub(1).max(1))),
            )
        } else {
            (
                format!(
                    "'{}': edge_distance {} is under the {} minimum edge distance for {}",
                    self.id,
                    fmt_ft_in(e),
                    fmt_ft_in(min),
                    self.size.text
                ),
                op_update(self.id, "edge_distance", "0.375"),
            )
        };
        out.warn("W_NAIL_EDGE", msg, fix);
    }
}

fn round_up_quarter(x: f64) -> f64 {
    math::ceil(x * 4.0 - 1e-9) / 4.0
}

fn same_kind_table(s: &Size) -> &'static [(u32, f64, f64)] {
    let t = s.text.to_ascii_lowercase();
    if t.ends_with("box") {
        &BOX
    } else if t.ends_with("sinker") {
        &SINKER
    } else {
        &COMMON
    }
}

fn is_penny(s: &Size) -> bool {
    !s.screw
        && s.text.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && s.text.to_ascii_lowercase().contains('d')
        && !s.text.contains(['x', 'X'])
}

/// The shortest penny size of the same kind at least `need` long.
fn longer_size(s: &Size, need: f64) -> Option<String> {
    if !is_penny(s) {
        return None;
    }
    let suffix = suffix_of(s);
    same_kind_table(s).iter().find(|e| e.2 + 1e-9 >= need).map(|e| format!("{}d{suffix}", e.0))
}

/// The longest penny size of the same kind between `need` and `max` long.
fn shorter_size(s: &Size, need: f64, max: f64) -> Option<String> {
    if !is_penny(s) {
        return None;
    }
    let suffix = suffix_of(s);
    same_kind_table(s)
        .iter()
        .rev()
        .find(|e| e.2 <= max + 1e-9 && e.2 + 1e-9 >= need)
        .map(|e| format!("{}d{suffix}", e.0))
}

fn suffix_of(s: &Size) -> &'static str {
    let t = s.text.to_ascii_lowercase();
    if t.ends_with("box") {
        " box"
    } else if t.ends_with("sinker") {
        " sinker"
    } else {
        ""
    }
}

// ---- patterns ----------------------------------------------------------------------------------------

/// The extent of the contiguous stack along `axis` beyond b's far face: members after the second one that continue it.
fn stack_after(job: &Job, a: &Member, axis: usize, side: f64, far: f64, at: [f64; 3]) -> (f64, Option<String>) {
    let mut far = far;
    let mut last = None;
    for g in job.groups.iter().skip(2) {
        for m in g {
            let (lo, hi) = box_in(a, m);
            let inside = (0..3).filter(|&o| o != axis).all(|o| at[o] > lo[o] + 1e-6 && at[o] < hi[o] - 1e-6);
            let near = if side > 0.0 { lo[axis] } else { hi[axis] };
            if inside && (near - far).abs() <= TOL {
                far = if side > 0.0 { hi[axis] } else { lo[axis] };
                last = Some(member_name(job.scene, m));
            }
        }
    }
    (far, last)
}

fn face_pattern(job: &Job, out: &mut Out) {
    let cp = job.cp;
    let rows = usize::from(cp.rows);
    let d = job.size.dia;
    let e = cp.edge_distance.unwrap_or(10.0 * d);
    job.check_edge(out, e, "ends");
    let Some(b_group) = job.groups.get(1) else { return };
    let mut any = false;
    let mut first_miss: Option<(Member, Member)> = None;
    for a in &job.groups[0] {
        for b in b_group {
            let (blo, bhi) = box_in(a, b);
            let Some(ct) = contact(a.lo, a.hi, blo, bhi) else {
                if first_miss.is_none() {
                    first_miss = Some((*a, *b));
                }
                continue;
            };
            any = true;
            let k = ct.axis;
            let others: Vec<usize> = (0..3).filter(|&o| o != k).collect();
            let len_of = |o: usize| ct.ov[o][1] - ct.ov[o][0];
            let mut r_axis = if len_of(others[1]) > len_of(others[0]) + 1e-9 { others[1] } else { others[0] };
            if let Some(w) = cp.along {
                let la = local_axis(a, w);
                if la != k {
                    r_axis = la;
                } else {
                    out.warn(
                        "W_PARAM",
                        format!("'{}': along {} is the direction the nails are driven, not a direction in the joint face; the rows follow the joint's longer side", job.id, w.tag()),
                        format!("drop \"along\" or name an axis of the joint face: {}", op_update(job.id, "along", &format!("\"{}\"", axis_letter_world(a, r_axis)))),
                    );
                }
            }
            let c_axis = if r_axis == others[0] { others[1] } else { others[0] };
            let mut r0 = ct.ov[r_axis][0];
            let mut r1 = ct.ov[r_axis][1];
            if let Some(rg) = job.range_on(a, r_axis) {
                r0 = r0.max(rg[0]);
                r1 = r1.min(rg[1]);
                if r1 <= r0 + 1e-9 {
                    out.warn(
                        "W_NAIL_MEMBERS",
                        format!(
                            "'{}': range misses the joint between '{}' and '{}'",
                            job.id,
                            member_name(job.scene, a),
                            member_name(job.scene, b)
                        ),
                        "set range to two points inside the joint, or drop it".to_string(),
                    );
                    continue;
                }
            }
            let (i0, i1) = if r1 - r0 > 2.0 * e { (r0 + e, r1 - e) } else { ((r0 + r1) / 2.0, (r0 + r1) / 2.0) };
            let stagger = cp.stagger && rows > 1;
            let positions: Vec<(f64, usize)> = match (cp.count.map(usize::from), cp.spacing) {
                (Some(n), Some(s)) => {
                    if stagger {
                        centered(i0, i1, n, s / rows as f64)
                            .into_iter()
                            .enumerate()
                            .map(|(q, x)| (x, q % rows))
                            .collect()
                    } else {
                        let per = n.div_ceil(rows);
                        expand_rows(&centered(i0, i1, per, s), rows, n)
                    }
                }
                (Some(n), None) => {
                    if stagger {
                        spread(i0, i1, n).into_iter().enumerate().map(|(q, x)| (x, q % rows)).collect()
                    } else {
                        let per = n.div_ceil(rows);
                        expand_rows(&spread(i0, i1, per), rows, n)
                    }
                }
                (None, s) => {
                    let s = s.unwrap_or(12.0);
                    let per = (math::floor((i1 - i0) / s + 1e-9) as usize + 1).min(5000);
                    if stagger {
                        let step = s / rows as f64;
                        let m = (math::floor((i1 - i0) / step + 1e-9) as usize + 1).min(5000);
                        centered(i0, i1, m, step).into_iter().enumerate().map(|(q, x)| (x, q % rows)).collect()
                    } else {
                        expand_rows(&centered(i0, i1, per, s), rows, per * rows)
                    }
                }
            };
            let row_pitch = row_spacing(&positions, rows);
            job.check_spacing(out, row_pitch, false);
            if let (Some(n), Some(s)) = (cp.count, cp.spacing) {
                let need = (f64::from(n).max(1.0) - 1.0) * if stagger { s / rows as f64 } else { s };
                if need > i1 - i0 + 1e-6 && need > 0.0 {
                    let fit = math::floor((i1 - i0) / if stagger { s / rows as f64 } else { s } + 1e-9) as u32 + 1;
                    out.warn(
                        "W_NAIL_SPACING",
                        format!(
                            "'{}': {n} nails @ {} need {} of joint but '{}' and '{}' share {} (less {} edge distance at each end)",
                            job.id,
                            fmt_ft_in(s),
                            fmt_ft_in(need),
                            member_name(job.scene, a),
                            member_name(job.scene, b),
                            fmt_ft_in(r1 - r0),
                            fmt_ft_in(e)
                        ),
                        op_update(job.id, "count", &format!("{}", fit.max(1))),
                    );
                }
            }
            let (c0, c1) = (ct.ov[c_axis][0], ct.ov[c_axis][1]);
            let across = |r: usize| c0 + (r as f64 + 1.0) * (c1 - c0) / (rows as f64 + 1.0);
            job.check_edge(out, (c1 - c0) / (rows as f64 + 1.0), "across");
            let head_k = if ct.side > 0.0 { a.lo[k] } else { a.hi[k] };
            let dir = unit(k, ct.side);
            let b_far = if ct.side > 0.0 { bhi[k] } else { blo[k] };
            let mut probe = [0.0; 3];
            probe[r_axis] = (i0 + i1) / 2.0;
            probe[c_axis] = (c0 + c1) / 2.0;
            let (far, extra) = stack_after(job, a, k, ct.side, b_far, probe);
            let total = (far - head_k).abs();
            let head = (a.hi[k] - a.lo[k]).abs();
            let bn = member_name(job.scene, b);
            let (held, last, before) = match extra {
                Some(name) => {
                    (format!("the stack '{bn}'..'{name}'"), name, last_before(job, a, k, ct.side, head_k, far, probe))
                }
                None => (format!("'{bn}'"), bn, head),
            };
            job.check_length(out, &held, &last, head, before, total);
            out.flip = total - before < job.size.len && !job.covered(a, k, ct.side, far, probe);
            for (x, r) in positions {
                let mut h = [0.0; 3];
                h[k] = head_k;
                h[r_axis] = x;
                h[c_axis] = across(r);
                out.push(job.nail(a, h, dir, unit(r_axis, 1.0), row_pitch));
            }
        }
    }
    if !any && let Some((a, b)) = first_miss {
        job.miss(out, &a, &b);
    }
}

/// Thickness ahead of the last member of a stack longer than two: the stack less the last member's own thickness.
fn last_before(job: &Job, a: &Member, axis: usize, side: f64, head: f64, far: f64, at: [f64; 3]) -> f64 {
    for g in job.groups.iter().skip(2) {
        for m in g {
            let (lo, hi) = box_in(a, m);
            let inside = (0..3).filter(|&o| o != axis).all(|o| at[o] > lo[o] + 1e-6 && at[o] < hi[o] - 1e-6);
            let end = if side > 0.0 { hi[axis] } else { lo[axis] };
            if inside && (end - far).abs() <= TOL {
                let near = if side > 0.0 { lo[axis] } else { hi[axis] };
                return (near - head).abs();
            }
        }
    }
    (far - head).abs()
}

fn axis_letter_world(a: &Member, axis: usize) -> &'static str {
    match face_name(a, axis, 1.0) {
        "left" | "right" => "x",
        "top" | "bottom" => "y",
        _ => "z",
    }
}

fn spread(a: f64, b: f64, n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![(a + b) / 2.0];
    }
    (0..n).map(|k| a + (b - a) * k as f64 / (n - 1) as f64).collect()
}

fn expand_rows(pos: &[f64], rows: usize, limit: usize) -> Vec<(f64, usize)> {
    let mut out = Vec::new();
    for &x in pos {
        for r in 0..rows {
            if out.len() < limit {
                out.push((x, r));
            }
        }
    }
    out
}

/// The smallest distance between consecutive nails of one row.
fn row_spacing(pos: &[(f64, usize)], rows: usize) -> f64 {
    let mut best = f64::INFINITY;
    for r in 0..rows {
        let xs: Vec<f64> = pos.iter().filter(|p| p.1 == r).map(|p| p.0).collect();
        for w in xs.windows(2) {
            best = best.min((w[1] - w[0]).abs());
        }
    }
    if best.is_finite() { best } else { 0.0 }
}

fn toenail_pattern(job: &Job, out: &mut Out, faces_in: &[String]) {
    let cp = job.cp;
    let len = job.size.len;
    let Some(b_group) = job.groups.get(1) else { return };
    let mut any = false;
    let mut first_miss: Option<(Member, Member)> = None;
    for a in &job.groups[0] {
        for b in b_group {
            let (blo, bhi) = box_in(a, b);
            let Some(ct) = contact(a.lo, a.hi, blo, bhi) else {
                if first_miss.is_none() {
                    first_miss = Some((*a, *b));
                }
                continue;
            };
            any = true;
            let k = ct.axis;
            let mut faces: Vec<(usize, f64)> = Vec::new();
            if faces_in.is_empty() {
                let n = if k == 0 { 1 } else { 0 };
                faces.push((n, -1.0));
                faces.push((n, 1.0));
            } else {
                for f in faces_in {
                    match axis_of_name(a, f) {
                        Some((ax, sg)) if ax != k => faces.push((ax, sg)),
                        _ => {
                            let side_faces: Vec<&str> = (0..3)
                                .filter(|&o| o != k)
                                .flat_map(|o| [face_name(a, o, -1.0), face_name(a, o, 1.0)])
                                .collect();
                            out.warn(
                                "W_PARAM",
                                format!("'{}': face '{f}' of '{}' is not a side face for a toenail into '{}' (it is the joint face or its opposite); side faces: {}", job.id, member_name(job.scene, a), member_name(job.scene, b), side_faces.join(", ")),
                                op_update(job.id, "faces", &quoted_list(&side_faces.iter().take(2).map(|s| s.to_string()).collect::<Vec<_>>())),
                            );
                        }
                    }
                }
            }
            if faces.is_empty() {
                continue;
            }
            let nf = faces.len();
            let w_of = |n: usize| (0..3).find(|&o| o != k && o != n).unwrap_or(2);
            let joint = if ct.side > 0.0 { a.hi[k] } else { a.lo[k] };
            let mut placed: Vec<(usize, f64)> = Vec::new();
            match (cp.count.map(usize::from), cp.spacing) {
                (count, Some(s)) => {
                    let w = w_of(faces[0].0);
                    let mut w0 = ct.ov[w][0];
                    let mut w1 = ct.ov[w][1];
                    if let Some(rg) = job.range_on(a, w) {
                        w0 = w0.max(rg[0]);
                        w1 = w1.min(rg[1]);
                    }
                    let n = count
                        .unwrap_or((math::floor((w1 - w0 - 2.0 * 0.375).max(0.0) / s + 1e-9) as usize + 1).min(5000));
                    for (q, x) in centered(w0, w1, n, s).into_iter().enumerate() {
                        placed.push((q % nf, x));
                    }
                }
                (count, None) => {
                    let n = count.unwrap_or(2);
                    for (m, _) in faces.iter().enumerate() {
                        let c_m = (0..n).filter(|q| q % nf == m).count();
                        let w = w_of(faces[m].0);
                        let (w0, w1) = (ct.ov[w][0], ct.ov[w][1]);
                        for q in 0..c_m {
                            placed.push((m, w0 + (q as f64 + 1.0) * (w1 - w0) / (c_m as f64 + 1.0)));
                        }
                    }
                }
            }
            let toe = Toe { a, k, side: ct.side, joint, t: a.hi[k] - a.lo[k], faces: &faces, w_of: &w_of };
            let mut bodies: Vec<([f64; 3], [f64; 3])> = vec![(blo, bhi)];
            bodies.extend(job.groups.iter().skip(2).flatten().map(|m| box_in(a, m)));
            for &(m, x) in &placed {
                let (h, dir) = toe.nail(m, x, len, cp.angle);
                let mut n = job.nail(a, h, dir, unit(w_of(faces[m].0), 1.0), cp.spacing.unwrap_or(0.0));
                n.face = dir_world(a, unit(faces[m].0, faces[m].1));
                out.push(n);
            }
            if !job.through {
                toenail_check(job, out, &toe, &placed, &bodies, b);
            }
        }
    }
    if !any && let Some((a, b)) = first_miss {
        job.miss(out, &a, &b);
    }
}

/// One toenailed joint: the first member `a`, the joint axis `k` and side, its thickness along k and the side faces.
struct Toe<'a> {
    a: &'a Member,
    k: usize,
    side: f64,
    joint: f64,
    t: f64,
    faces: &'a [(usize, f64)],
    w_of: &'a dyn Fn(usize) -> usize,
}

impl Toe<'_> {
    /// Head and unit direction of a toenail of length `len` at `angle` on face `m`, at `x` along the joint: started
    /// min(len/3, 3/4 of the member) back from the joint.
    fn nail(&self, m: usize, x: f64, len: f64, angle: f64) -> ([f64; 3], [f64; 3]) {
        let (n_axis, n_sign) = self.faces[m];
        let th = angle.to_radians();
        let back = (len / 3.0).min(self.t * 0.75);
        let mut h = [0.0; 3];
        h[self.k] = self.joint - self.side * back;
        h[n_axis] = if n_sign > 0.0 { self.a.hi[n_axis] } else { self.a.lo[n_axis] };
        h[(self.w_of)(n_axis)] = x;
        let mut dir = [0.0; 3];
        dir[self.k] = math::cos(th) * self.side;
        dir[n_axis] = -math::sin(th) * n_sign;
        (h, dir)
    }

    /// The nails (indices into `placed`) whose points end outside every box of `bodies`.
    fn outside(&self, placed: &[(usize, f64)], len: f64, angle: f64, bodies: &[([f64; 3], [f64; 3])]) -> Vec<usize> {
        (0..placed.len())
            .filter(|&q| {
                let (m, x) = placed[q];
                let (h, d) = self.nail(m, x, len, angle);
                let tip = [h[0] + d[0] * len, h[1] + d[1] * len, h[2] + d[2] * len];
                !bodies.iter().any(|(lo, hi)| (0..3).all(|o| tip[o] >= lo[o] - TOL && tip[o] <= hi[o] + TOL))
            })
            .collect()
    }
}

/// W_NAIL_LENGTH for toenails whose points leave the members. Every fix offered is checked with the same geometry (head
/// started len/3 back, the size's own length, the angle), so applying one clears the warning.
fn toenail_check(
    job: &Job,
    out: &mut Out,
    toe: &Toe,
    placed: &[(usize, f64)],
    bodies: &[([f64; 3], [f64; 3])],
    b: &Member,
) {
    let cp = job.cp;
    let a = toe.a;
    let len = job.size.len;
    let bad = toe.outside(placed, len, cp.angle, bodies);
    let Some(&first) = bad.first() else { return };
    let (m, x) = placed[first];
    let (h, d) = toe.nail(m, x, len, cp.angle);
    let tip = [h[0] + d[0] * len, h[1] + d[1] * len, h[2] + d[2] * len];
    let (blo, bhi) = bodies[0];
    let mut worst = (0usize, 0.0f64, 1.0f64);
    for o in 0..3 {
        for (gap, sign) in [(blo[o] - tip[o], -1.0), (tip[o] - bhi[o], 1.0)] {
            if gap > worst.1 {
                worst = (o, gap, sign);
            }
        }
    }
    let bn = member_name(job.scene, b);
    let an = member_name(job.scene, a);
    let face_of = |m: usize| face_name(a, toe.faces[m].0, toe.faces[m].1);
    let mut bad_faces: Vec<&str> = Vec::new();
    for &q in &bad {
        let f = face_of(placed[q].0);
        if !bad_faces.contains(&f) {
            bad_faces.push(f);
        }
    }
    let msg = format!(
        "'{}': {} of {} {} toenails (from the {} face of '{an}', started {} from its end at {} deg) end outside '{bn}' and the other members: the point is {} past the {} face of '{bn}'",
        job.id,
        bad.len(),
        placed.len(),
        job.size.text,
        bad_faces.join(" and "),
        fmt_ft_in((len / 3.0).min(toe.t * 0.75)),
        fmt_num(cp.angle),
        fmt_ft_in(worst.1),
        face_name(a, worst.0, worst.2)
    );
    let mut fixes: Vec<String> = Vec::new();
    let mut more: Vec<([f64; 3], [f64; 3])> = bodies.to_vec();
    let mut extra: Vec<String> = Vec::new();
    for &q in &bad {
        let (m, x) = placed[q];
        let (h, d) = toe.nail(m, x, len, cp.angle);
        let tip = [h[0] + d[0] * len, h[1] + d[1] * len, h[2] + d[2] * len];
        if let Some((name, lo, hi)) = solid_at(job, a, tip) {
            if !extra.contains(&name) {
                extra.push(name);
            }
            more.push((lo, hi));
        }
    }
    let members_with = |names: &[String]| {
        let mut ms = job.members_json();
        for n in names {
            if !ms.contains(n) {
                ms.push(n.clone());
            }
        }
        quoted_list(&ms)
    };
    let still = toe.outside(placed, len, cp.angle, &more);
    let ok_faces: Vec<String> = (0..toe.faces.len())
        .filter(|&f| placed.iter().any(|p| p.0 == f) && !still.iter().any(|&q| placed[q].0 == f))
        .map(|f| face_of(f).to_string())
        .collect();
    if !extra.is_empty() {
        if still.is_empty() {
            fixes.push(format!(
                "the points end in {}: list it after '{bn}': {}",
                scene::join_ids(&extra),
                op_update(job.id, "members", &members_with(&extra))
            ));
        } else if !ok_faces.is_empty() {
            fixes.push(format!(
                "some points end in {}: list it and nail from the {} face only: {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"members\":{},\"faces\":{}}}}}",
                scene::join_ids(&extra),
                ok_faces.join(" and "),
                job.id,
                members_with(&extra),
                quoted_list(&ok_faces)
            ));
        }
    } else if !ok_faces.is_empty() {
        fixes.push(format!(
            "nail from the {} face only: {}",
            ok_faces.join(" and "),
            op_update(job.id, "faces", &quoted_list(&ok_faces))
        ));
    }
    let mut angles: Vec<f64> = (3..=12).map(|s| f64::from(s) * 5.0).filter(|g| (g - cp.angle).abs() > 1e-9).collect();
    crate::sort::stable(&mut angles, |x, y| (x - cp.angle).abs() < (y - cp.angle).abs());
    if let Some(g) = angles.iter().find(|&&g| toe.outside(placed, len, g, bodies).is_empty()) {
        fixes.push(format!(
            "{} at {} deg stays inside: {}",
            job.size.text,
            fmt_num(*g),
            op_update(job.id, "angle", &fmt_num(*g))
        ));
    }
    if is_penny(job.size) {
        let suffix = suffix_of(job.size);
        if let Some(e) = same_kind_table(job.size)
            .iter()
            .rev()
            .find(|e| (e.2 - len).abs() > 1e-9 && toe.outside(placed, e.2, cp.angle, bodies).is_empty())
        {
            let f = format!("{}d{suffix}", e.0);
            fixes.push(format!(
                "{f} stays inside at {} deg: {}",
                fmt_num(cp.angle),
                op_update(job.id, "size", &format!("\"{f}\""))
            ));
        }
    }
    fixes.push(format!(
        "where the schedule calls for this nail and the points may come through: {}",
        op_update(job.id, "through", "true")
    ));
    out.warn("W_NAIL_LENGTH", msg, fixes.join("; or "));
}

/// The first drawn solid (not a member, not hardware, not a nailing) whose box holds `p` (in `a`'s frame): its name and
/// box.
fn solid_at(job: &Job, a: &Member, p: [f64; 3]) -> Option<(String, [f64; 3], [f64; 3])> {
    let listed: Vec<u32> = job.groups.iter().flat_map(|g| g.iter().map(|m| m.comp)).collect();
    for c in &job.scene.comps {
        if c.state != State::Ok || listed.contains(&c.index) || c.built.prisms.is_empty() || !job.shares_view(c) {
            continue;
        }
        if c.ty.ty == crate::catalog::Type::Nailing || c.embedded || c.ty.traits.hardware {
            continue;
        }
        for m in members_of(c, None) {
            let (lo, hi) = box_in(a, &m);
            if (0..3).all(|o| p[o] >= lo[o] - TOL && p[o] <= hi[o] + TOL) {
                return Some((member_name(job.scene, &m), lo, hi));
            }
        }
    }
    None
}

/// A framing member behind a panel and the low / high corners of its box clipped to the panel (`box_in`).
type Backing = (Member, [f64; 3], [f64; 3]);

/// Framing behind a panel: the instances of the members after the sheets touching its thin axis, all on one side (the
/// side of the first).
fn framing(job: &Job, a: &Member, k: usize) -> (f64, Vec<Backing>) {
    let mut side = 0.0;
    let mut out = Vec::new();
    for g in job.groups.iter().skip(job.sheets) {
        for m in g {
            let (lo, hi) = box_in(a, m);
            let s = if (a.lo[k] - hi[k]).abs() <= TOL {
                -1.0
            } else if (a.hi[k] - lo[k]).abs() <= TOL {
                1.0
            } else {
                continue;
            };
            let ov = (0..3).filter(|&o| o != k).all(|o| a.hi[o].min(hi[o]) - a.lo[o].max(lo[o]) > TOL);
            if !ov {
                continue;
            }
            if side == 0.0 {
                side = s;
            }
            if s == side {
                out.push((*m, lo, hi));
            }
        }
    }
    (side, out)
}

fn panel_axis(a: &Member) -> usize {
    let ext = [a.hi[0] - a.lo[0], a.hi[1] - a.lo[1], a.hi[2] - a.lo[2]];
    let mut k = 0;
    for o in 1..3 {
        if ext[o] < ext[k] - 1e-9 {
            k = o;
        }
    }
    k
}

/// Edge and field: the leading `panel` members are the sheets side by side (at least the first; at least one member is
/// left for the framing). A panel that lies against the thin face of an earlier sheet is backing, not another sheet.
fn sheet_count(groups: &[Vec<Member>], panels: &[bool]) -> usize {
    let mut n = 1;
    while n + 1 < groups.len() && panels[n] {
        let stacked = groups[..n].iter().flatten().any(|a| {
            let k = panel_axis(a);
            groups[n].iter().any(|m| {
                let (lo, hi) = box_in(a, m);
                let touches = (a.lo[k] - hi[k]).abs() <= TOL || (a.hi[k] - lo[k]).abs() <= TOL;
                touches && (0..3).filter(|&o| o != k).all(|o| a.hi[o].min(hi[o]) - a.lo[o].max(lo[o]) > TOL)
            })
        });
        if stacked {
            break;
        }
        n += 1;
    }
    n
}

fn panel_pattern(job: &Job, out: &mut Out, edges_in: &[String], field: bool) {
    let cp = job.cp;
    let e = cp.edge_distance.unwrap_or(0.375);
    let s = cp.spacing.unwrap_or(if field { 12.0 } else { 6.0 });
    job.check_edge(out, e, "panel");
    job.check_spacing(out, s, true);
    for a in job.groups[..job.sheets].iter().flatten() {
        let k = panel_axis(a);
        let (side, frames) = framing(job, a, k);
        if frames.is_empty() {
            let near = job.touching(a);
            let an = member_name(job.scene, a);
            let fix = if near.is_empty() {
                format!("list the studs, plates and blocking behind '{an}' after it in members")
            } else {
                let mut m = job.members_json();
                m.truncate(job.sheets);
                m.extend(near.iter().cloned());
                format!(
                    "members touching '{an}': {}. E.g. {}",
                    scene::join_ids(&near),
                    op_update(job.id, "members", &quoted_list(&m))
                )
            };
            out.warn(
                "W_NAIL_MEMBERS",
                format!(
                    "'{}': none of the framing members touches the face of panel '{an}', so no {} nailing is drawn",
                    job.id,
                    if field { "field" } else { "edge" }
                ),
                fix,
            );
            continue;
        }
        let head_k = if side > 0.0 { a.lo[k] } else { a.hi[k] };
        let dir = unit(k, side);
        let (i, j) = if k == 0 {
            (1, 2)
        } else if k == 1 {
            (0, 2)
        } else {
            (0, 1)
        };
        let backed = |p: [f64; 3]| {
            frames
                .iter()
                .find(|(_, lo, hi)| (0..3).filter(|&o| o != k).all(|o| p[o] > lo[o] + 1e-6 && p[o] < hi[o] - 1e-6))
        };
        let t_panel = a.hi[k] - a.lo[k];
        let mut checked_len = false;
        let mut emit = |out: &mut Out, h: [f64; 3], row: usize, pitch: f64, fr: &(Member, [f64; 3], [f64; 3])| {
            if !checked_len {
                checked_len = true;
                let first = member_name(job.scene, &fr.0);
                let mut far = if side > 0.0 { fr.2[k] } else { fr.1[k] };
                let mut last = first.clone();
                let mut grown = true;
                while grown {
                    grown = false;
                    for m in job.groups[job.sheets..].iter().flatten() {
                        let (lo, hi) = box_in(a, m);
                        let start = if side > 0.0 { lo[k] } else { hi[k] };
                        let end = if side > 0.0 { hi[k] } else { lo[k] };
                        let inside = (0..3).filter(|&o| o != k).all(|o| h[o] > lo[o] + 1e-6 && h[o] < hi[o] - 1e-6);
                        if inside && (start - far).abs() <= TOL && (end - far) * side > 1e-9 {
                            far = end;
                            last = member_name(job.scene, m);
                            grown = true;
                        }
                    }
                }
                job.check_length(out, &format!("'{first}'"), &last, t_panel, t_panel, (far - head_k).abs());
            }
            out.push(job.nail(a, h, dir, unit(row, 1.0), pitch));
        };
        if !field {
            for (q, p) in [(i, j), (j, i)] {
                for sign in [-1.0, 1.0] {
                    let name = face_name(a, q, sign);
                    if !edges_in.is_empty() && !edges_in.iter().any(|x| x == name) {
                        continue;
                    }
                    let qc = if sign > 0.0 { a.hi[q] - e } else { a.lo[q] + e };
                    let pos = fill_span(a.lo[p] + e, a.hi[p] - e, s);
                    let step = if pos.len() > 1 { pos[1] - pos[0] } else { 0.0 };
                    let mut gaps: Vec<[f64; 2]> = Vec::new();
                    let mut gap_pts: Vec<[f64; 3]> = Vec::new();
                    for x in pos {
                        let mut h = [0.0; 3];
                        h[k] = head_k;
                        h[q] = qc;
                        h[p] = x;
                        match backed(h) {
                            Some(fr) => emit(out, h, p, step, fr),
                            None if !cp.blocked => {}
                            None => {
                                gap_pts.push(h);
                                match gaps.last_mut() {
                                    Some(g) if (x - g[1] - step).abs() < 1e-6 => g[1] = x,
                                    _ => gaps.push([x, x]),
                                }
                            }
                        }
                    }
                    if !gaps.is_empty() {
                        let an = member_name(job.scene, a);
                        let pw = to_world(a, {
                            let mut v = [0.0; 3];
                            v[k] = head_k;
                            v[q] = qc;
                            v[p] = gaps[0][0];
                            v
                        });
                        let near = job.backing(a, k, &gap_pts);
                        let mut fix =
                            format!("put blocking or framing under the {name} edge of '{an}' and list it in members");
                        if !near.is_empty() {
                            let mut m = job.members_json();
                            for n in &near {
                                if !m.contains(n) {
                                    m.push(n.clone());
                                }
                            }
                            fix = format!(
                                "'{}' lie behind that edge: {}; or {fix}",
                                scene::join_ids(&near),
                                op_update(job.id, "members", &quoted_list(&m))
                            );
                        }
                        let others: Vec<String> = ["left", "right", "top", "bottom", "front", "back"]
                            .iter()
                            .filter(|n| {
                                **n != name
                                    && (0..3)
                                        .filter(|&o| o != k)
                                        .any(|o| face_name(a, o, -1.0) == **n || face_name(a, o, 1.0) == **n)
                            })
                            .filter(|n| edges_in.is_empty() || edges_in.iter().any(|x| x == *n))
                            .map(|n| n.to_string())
                            .collect();
                        if others.is_empty() {
                            fix.push_str(&format!(
                                "; or, if that edge stays unnailed, delete '{}' (it nails only that edge)",
                                job.id
                            ));
                        } else {
                            fix.push_str(&format!(
                                "; or leave that edge unnailed: {}",
                                op_update(job.id, "edges", &quoted_list(&others))
                            ));
                        }
                        fix.push_str(&format!(
                            "; or, for an unblocked diaphragm or wall (nails only where framing crosses the edge): {}",
                            op_update(job.id, "blocked", "false")
                        ));
                        out.warn(
                            "W_NAIL_EDGE",
                            format!(
                                "'{}': the {name} edge of '{an}' has no framing behind it for {} of its length (first gap at [{}, {}, {}]); panel edges are nailed into framing or blocking",
                                job.id,
                                fmt_ft_in(gaps.iter().map(|g| g[1] - g[0] + step).sum::<f64>()),
                                fmt_num(pw[0]),
                                fmt_num(pw[1]),
                                fmt_num(pw[2])
                            ),
                            fix,
                        );
                    }
                }
            }
        } else {
            let mut rows_found = 0;
            for fr in &frames {
                let (lo, hi) = (fr.1, fr.2);
                let r = |o: usize| [lo[o].max(a.lo[o]), hi[o].min(a.hi[o])];
                let (p, q) = if r(i)[1] - r(i)[0] >= r(j)[1] - r(j)[0] { (i, j) } else { (j, i) };
                let rq = r(q);
                let mid = (rq[0] + rq[1]) / 2.0;
                let under_edge =
                    [a.lo[q] + e, a.hi[q] - e].iter().any(|&line| line > rq[0] - 1e-6 && line < rq[1] + 1e-6)
                        || mid - a.lo[q] < EDGE_ZONE
                        || a.hi[q] - mid < EDGE_ZONE;
                if under_edge {
                    continue;
                }
                rows_found += 1;
                let rp = r(p);
                let (lo_row, hi_row) = (a.lo[p] + e, a.hi[p] - e);
                let from = if rp[0] < lo_row + FIELD_CLEAR { lo_row } else { rp[0] };
                let to = if rp[1] > hi_row - FIELD_CLEAR { hi_row } else { rp[1] };
                let pos = fill_span(from, to, s);
                let step = if pos.len() > 1 { pos[1] - pos[0] } else { 0.0 };
                for x in pos {
                    if x < a.lo[p] + e + FIELD_CLEAR - 1e-6 || x > a.hi[p] - e - FIELD_CLEAR + 1e-6 {
                        continue;
                    }
                    let mut h = [0.0; 3];
                    h[k] = head_k;
                    h[q] = (rq[0] + rq[1]) / 2.0;
                    h[p] = x;
                    emit(out, h, p, step, fr);
                }
            }
            if rows_found == 0 {
                out.warn(
                    "W_NAIL_MEMBERS",
                    format!("'{}': every framing member behind '{}' lies under a panel edge, so there is no field nailing to draw", job.id, member_name(job.scene, a)),
                    "list the intermediate studs or joists after the panel in members (members under panel edges get the edge nailing)".to_string(),
                );
            }
        }
    }
}

// ---- builder -----------------------------------------------------------------------------------------

fn string_list(ctx: &mut Ctx, key: &str, v: Option<&Value>, allowed: &[&str]) -> Option<Vec<String>> {
    let Some(v) = v else { return Some(Vec::new()) };
    let Some(arr) = v.arr() else {
        ctx.p
            .fail(key, format!("param '{key}' must be a list of names, e.g. [\"{}\", \"{}\"]", allowed[0], allowed[1]));
        return None;
    };
    let mut out = Vec::new();
    for (i, e) in arr.iter().enumerate() {
        match e.str() {
            Some(s) if allowed.contains(&s) => out.push(s.to_string()),
            _ => {
                ctx.p.fail(
                    &format!("{key}/{i}"),
                    format!(
                        "{key}[{i}] must be one of {} (got {})",
                        model::join_quoted(allowed),
                        model::kind_or_text(e)
                    ),
                );
                return None;
            }
        }
    }
    Some(out)
}

fn resolve_range(ctx: &mut Ctx, v: &Value) -> Option<[RangeEnd; 2]> {
    let fail_msg = "param 'range' must be [from, to]: two Refs (\"plate@top_right\") or [x, y] points (the rows run between them), or two z values for rows along z";
    let Some(arr) = v.arr().filter(|a| a.len() == 2) else {
        ctx.p.fail("range", fail_msg.into());
        return None;
    };
    let mut out = [RangeEnd::Z(0.0); 2];
    for (k, e) in arr.iter().enumerate() {
        let path = format!("{}/{}/range/{k}", ctx.p.base, ctx.p.id);
        out[k] = match e {
            Value::Number(_) => RangeEnd::Z(crate::units::parse_length(e)?),
            Value::String(s) if crate::units::parse_length_str(s).is_some() && !s.contains('@') => {
                RangeEnd::Z(crate::units::parse_length_str(s)?)
            }
            _ => match scene::point_from_value(ctx.scene, e, ctx.p.id, &path) {
                Some(p) => RangeEnd::Pt(p),
                None => {
                    ctx.p.ok = false;
                    return None;
                }
            },
        };
    }
    Some(out)
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    for key in ["at", "rotate", "slope", "mirror", "array", "z"] {
        if ctx.p.has(key) {
            ctx.p.fail(key, format!("a nailing is placed from its members; remove \"{key}\" (`range` limits where the rows go, `views` which views show it)"));
        }
    }
    let cp = ctx.p.parse_all::<Params>();
    if !ctx.p.ok {
        return None;
    }
    let Some(size) = parse_size(&cp.size, cp.fastener == Fastener::Screw) else {
        ctx.p.fail("size", format!("size \"{}\" is not a fastener size. Use {SIZE_FORMS}", cp.size));
        return None;
    };
    let Some(mv) = cp.members.as_ref() else {
        ctx.p.fail("members", "nailing needs \"members\": the ids it joins, the member the heads are on first, e.g. [\"top_plate_upper\", \"top_plate_lower\"]".into());
        return None;
    };
    let ids: Vec<&Value> = mv.arr().map(|a| a.iter().collect()).unwrap_or_default();
    if ids.len() < 2 || ids.iter().any(|v| v.str().is_none()) {
        ctx.p.fail(
            "members",
            format!(
                "\"members\" must list at least two component ids (got {}), e.g. [\"sheathing\", \"stud\", \"plate\"]",
                model::kind_or_text(mv)
            ),
        );
        return None;
    }
    let scene = ctx.scene;
    let mut groups: Vec<Vec<Member>> = Vec::new();
    let mut panels: Vec<bool> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    for (i, v) in ids.iter().enumerate() {
        let s = v.str().unwrap_or("");
        let (cid, inst) = match s.split_once('#') {
            Some((c, k)) => (c, k.parse::<usize>().ok()),
            None => (s, None),
        };
        let path = format!("members/{i}");
        let Some(c) = scene.find(cid) else {
            let ids = scene.comp_ids();
            let hint = match model::nearest(cid, &ids) {
                Some(n) => format!(" Did you mean '{n}'?"),
                None => format!(" Components: {}", scene::join_ids(&ids)),
            };
            ctx.p.fail_code("E_REF_UNKNOWN", &path, format!("member '{s}' is not a component.{hint}"));
            return None;
        };
        if c.id == ctx.p.id || c.ty.ty == crate::catalog::Type::Nailing {
            ctx.p.fail(&path, format!("member '{s}' is a nailing; members are the pieces the fasteners go through"));
            return None;
        }
        if c.state != State::Ok {
            ctx.p.ok = false;
            return None;
        }
        if let Some(k) = inst
            && k >= c.xfs.len()
        {
            ctx.p.fail(
                &path,
                format!("'{cid}' has {} instance(s) (#0..#{}); got #{k}", c.xfs.len(), c.xfs.len().saturating_sub(1)),
            );
            return None;
        }
        if s.contains('#') && inst.is_none() {
            ctx.p.fail(&path, format!("member '{s}': write an instance as id#k with k a number, e.g. \"{cid}#0\""));
            return None;
        }
        groups.push(members_of(c, inst));
        panels.push(c.ty.ty == crate::catalog::Type::Panel);
        if !stack.contains(&c.index) {
            stack.push(c.index);
        }
    }
    let edges = string_list(ctx, "edges", cp.edges.as_ref(), &FACE_NAMES)?;
    let faces = string_list(ctx, "faces", cp.faces.as_ref(), &FACE_NAMES)?;
    let range_pts = match cp.range.as_ref() {
        Some(v) => Some(resolve_range(ctx, v)?),
        None => None,
    };
    if cp.pattern == Pattern::Face && cp.count.is_none() && cp.spacing.is_none() {
        ctx.p.fail(
            "count",
            "face nailing needs \"count\" (nails per joint, e.g. 8) or \"spacing\" (o.c., e.g. 12)".into(),
        );
        return None;
    }
    if !(5.0..=85.0).contains(&cp.angle) {
        ctx.p.fail("angle", format!("angle must be between 5 and 85 degrees (got {})", fmt_num(cp.angle)));
        return None;
    }
    let sheets = if matches!(cp.pattern, Pattern::Edge | Pattern::Field) { sheet_count(&groups, &panels) } else { 1 };
    let views = ctx.p.raw("views");
    let job = Job { id: ctx.p.id, size: &size, cp: &cp, through: cp.through, scene, groups, sheets, range_pts, views };
    let mut out = Out { nails: Vec::new(), warns: Vec::new(), flip: false };
    match cp.pattern {
        Pattern::Face => face_pattern(&job, &mut out),
        Pattern::Toenail => toenail_pattern(&job, &mut out, &faces),
        Pattern::Edge => panel_pattern(&job, &mut out, &edges, false),
        Pattern::Field => panel_pattern(&job, &mut out, &edges, true),
    }
    if out.nails.len() > crate::limits::MAX_POINTS {
        ctx.p.fail_code(
            "E_LIMIT",
            "spacing",
            format!("{} fasteners is over the limit of {}; draw a representative panel or a wider spacing and say the schedule in a note", out.nails.len(), crate::limits::MAX_POINTS),
        );
        return None;
    }
    let path = format!("components/{}", ctx.p.id);
    for w in &out.warns {
        ctx.p.diags.add_fix(Level::Warning, w.code, Some(ctx.p.id), Some(&path), w.msg.clone(), w.fix.clone());
    }
    let mut bx = Box::default();
    for n in &out.nails {
        bx.add_point(n.head[0], n.head[1]);
        bx.add_point(n.tip[0], n.tip[1]);
    }
    if bx.is_empty() {
        bx = Box { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
    }
    let text = if cp.text.trim_matches(' ').is_empty() {
        text_form(&TextIn {
            pattern: cp.pattern,
            size: &cp.size,
            screw: size.screw,
            count: cp.count.map(u32::from),
            spacing: cp.spacing,
            rows: u32::from(cp.rows),
            stagger: cp.stagger,
        })
    } else {
        cp.text.clone()
    };
    let names: Vec<&str> = ids.iter().filter_map(|v| v.str()).collect();
    let info = format!(
        "nailing {} {}: {} {} ({} x {}), {}",
        cp.pattern.tag(),
        text,
        out.nails.len(),
        if size.screw { "screws" } else { "nails" },
        fmt_num(size.dia),
        fmt_ft_in(size.len),
        names.join(" -> ")
    );
    Some(Built {
        box_: bx,
        points_mode: true,
        nails: out.nails,
        nail_stack: stack,
        nail_flip: out.flip,
        info,
        ..Built::default()
    })
}
