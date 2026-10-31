//! Lumber and steel cuts and two-point members (SPEC 27.3). A member seen lengthwise (lumber run x/y, steel in elevation
//! or plan) is a strip in its frame: u along the length (0 at the start, `length` at the end), v across (0 at the bottom
//! face, h at the top; v is always to the left of u). Its two end cuts are lines that close the strip; the birdmouth and
//! rectangular notches are polygons taken out of it. Everything happens in the builder's local coordinates before the
//! instances are placed, so sections, plans, elevations, iso, the mesh and every check see the cut profile. A notch `by`
//! another member is a subtraction after placement (`voids`).

use crate::catalog::Type;
use crate::clip::{self, Op};
use crate::geom::{self, Box, Pt, V2, Xf};
use crate::json::{self, Member, Value};
use crate::math;
use crate::model::{self, Built, Kind, Level, NamedAnchor, Prism};
use crate::scene::{self, Comp, Scene, State};
use crate::section::group_region;
use crate::units::{self, fmt_ft_in};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const FLAT_TOL: f64 = geom::FLAT_TOL;
/// How close a seat must sit on its bearing member's top (inches).
const SEAT_TOL: f64 = 1.0 / 32.0;
const TYPES: [&str; 7] = ["square", "plumb", "level", "bevel", "trim", "birdmouth", "notch"];
const CUTS_FIX: &str = "\"cuts\": [{\"type\": \"birdmouth\", \"on\": \"top_plate\", \"tail\": 12}, {\"type\": \"plumb\", \"end\": \"start\"}, {\"type\": \"trim\", \"end\": \"end\", \"to\": \"ridge\"}, {\"type\": \"notch\", \"at\": \"top_right\", \"w\": 3.5, \"d\": 1.5}, {\"type\": \"notch\", \"by\": \"top_plate\"}] (`kerf schema cuts`)";
const PLACE_KEYS: [&str; 6] = ["at", "length", "until", "rotate", "slope", "mirror"];

/// The frame of a member seen lengthwise in its builder's local coordinates: run x: u = +x, v = +y; run y: u = +y,
/// v = -x.
#[derive(Clone, Copy)]
struct Frame {
    o: V2,
    u: V2,
    v: V2,
    len: f64,
    h: f64,
}

impl Frame {
    fn of(b: Box, run_y: bool) -> Frame {
        if run_y {
            Frame {
                o: V2::init(b.x1, b.y0),
                u: V2::init(0.0, 1.0),
                v: V2::init(-1.0, 0.0),
                len: b.height(),
                h: b.width(),
            }
        } else {
            Frame {
                o: V2::init(b.x0, b.y0),
                u: V2::init(1.0, 0.0),
                v: V2::init(0.0, 1.0),
                len: b.width(),
                h: b.height(),
            }
        }
    }
    fn local(&self, f: V2) -> V2 {
        self.o.add(self.u.scale(f.x)).add(self.v.scale(f.y))
    }
    fn dir(&self, d: V2) -> V2 {
        V2::init(d.dot(self.u), d.dot(self.v))
    }
    fn local_xf(&self) -> Xf {
        Xf { a: self.u.x, b: self.u.y, c: self.v.x, d: self.v.y, tx: self.o.x, ty: self.o.y }
    }
    fn frame_xf(&self) -> Xf {
        Xf { a: self.u.x, b: self.v.x, c: self.u.y, d: self.v.y, tx: -self.o.dot(self.u), ty: -self.o.dot(self.v) }
    }
}

fn inverse(x: &Xf) -> Xf {
    let det = x.det();
    let (a, b, c, d) = (x.d / det, -x.b / det, -x.c / det, x.a / det);
    Xf { a, b, c, d, tx: -(a * x.tx + c * x.ty), ty: -(b * x.tx + d * x.ty) }
}

fn rot(angle: f64, p: V2) -> V2 {
    let (s, c) = (math::sin(angle), math::cos(angle));
    V2::init(c * p.x - s * p.y, s * p.x + c * p.y)
}

fn deg(a: f64) -> String {
    model::fmt_num(math::round(a * geom::DEG_PER_RAD * 10.0) / 10.0)
}

fn err(scene: &Scene, id: &str, path: &str, msg: String, fix: &str) {
    scene.diags.add_fix(Level::Error, "E_PARAM", Some(id), Some(path), msg, fix.to_string());
}

fn op_text(id: &str, value: Value) -> String {
    let mut buf = Vec::new();
    json::write_compact(
        &mut buf,
        &Value::Object(vec![
            Member { key: "op".into(), value: json::string("update") },
            Member { key: "path".into(), value: json::string(&format!("components/{id}")) },
            Member { key: "value".into(), value },
        ]),
    );
    String::from_utf8_lossy(&buf).into_owned()
}

fn obj1(key: &str, v: Value) -> Value {
    Value::Object(vec![Member { key: key.into(), value: v }])
}

/// The update op that sets cut `k` of `node`'s `cuts` to `entry` (or drops it when None).
fn cuts_op(id: &str, node: &Value, k: usize, entry: Option<Value>) -> String {
    let list: Vec<Value> = node
        .get("cuts")
        .and_then(Value::arr)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .filter_map(|(i, e)| if i == k { entry.clone() } else { Some(e.clone()) })
        .collect();
    op_text(id, obj1("cuts", Value::Array(list)))
}

/// Round down to 1/16".
fn sixteenth_floor(x: f64) -> f64 {
    math::floor(x * 16.0 + 1e-9) / 16.0
}

// ---- dependencies ----------------------------------------------------------------------------------------------

/// The components a member's `from`, `to` and `cuts` (birdmouth `on`, trim `to`, bevel `line`) name: placed first.
/// A notch `by` is not a dependency (it is subtracted after placement, like `voids`).
pub fn collect_deps<'a>(node: &'a Value, out: &mut Vec<&'a str>, add_ref: impl Fn(&'a Value, &mut Vec<&'a str>)) {
    for key in ["from", "to"] {
        if let Some(v) = node.get(key) {
            add_ref(v, out);
        }
    }
    for c in node.get("cuts").and_then(Value::arr).unwrap_or(&[]) {
        for key in ["on", "to"] {
            if let Some(s) = c.get(key).and_then(Value::str) {
                out.push(s.split(['#', '.']).next().unwrap_or(s));
            }
        }
        for p in c.get("line").and_then(Value::arr).unwrap_or(&[]) {
            add_ref(p, out);
        }
    }
}

/// The components a member's notches `by` name (refused removal targets, like `voids`).
pub fn collect_notch_refs<'a>(node: &'a Value, out: &mut Vec<&'a str>) {
    for c in node.get("cuts").and_then(Value::arr).unwrap_or(&[]) {
        if let Some(s) = c.get("by").and_then(Value::str) {
            out.push(s.split('.').next().unwrap_or(s));
        }
    }
}

/// `(cut index, member id)` of every `{"type": "notch", "by": id}` in a lumber or steel member's `cuts`.
pub fn notch_bys<'a>(c: &Comp<'a>) -> Vec<(usize, &'a str)> {
    if !matches!(c.ty.ty, Type::Lumber | Type::Steel) {
        return Vec::new();
    }
    let list = c.node.get("cuts").and_then(Value::arr).unwrap_or(&[]);
    list.iter()
        .enumerate()
        .filter(|(_, e)| e.get("type").and_then(Value::str) == Some("notch"))
        .filter_map(|(k, e)| e.get("by").and_then(Value::str).map(|s| (k, s)))
        .collect()
}

// ---- two-point members -----------------------------------------------------------------------------------------

#[derive(Clone)]
struct Target {
    comp: String,
    inst: u32,
    part: Option<String>,
}

pub struct TwoPoint {
    pub p0: V2,
    pub len: f64,
    pub angle: f64,
    align: f64,
    ends: [Option<Target>; 2],
    names: [String; 2],
}

fn ref_target(v: &Value) -> Option<Target> {
    let s = match v {
        Value::String(s) => s.as_str(),
        Value::Object(_) => v.get("ref")?.str()?,
        _ => return None,
    };
    let r = scene::parse_ref(s).ok()?;
    if r.origin {
        return None;
    }
    Some(Target { comp: r.comp.to_string(), inst: r.inst, part: r.part.map(String::from) })
}

fn point_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => {
            let r = v.get("ref").and_then(Value::str).unwrap_or("ref");
            match v.get("offset").and_then(Value::arr) {
                Some([dx, dy]) => format!(
                    "{r}+[{}, {}]",
                    units::parse_length(dx).map_or_else(|| "?".into(), model::fmt_num),
                    units::parse_length(dy).map_or_else(|| "?".into(), model::fmt_num)
                ),
                _ => r.to_string(),
            }
        }
        _ => model::kind_or_text(v),
    }
}

/// Whether the member is seen lengthwise, and then whether its length runs along y. None: cuts do not apply.
pub fn lengthwise(ty: Type, node: &Value, two_point: bool) -> Option<bool> {
    let run = node.get("run").and_then(Value::str);
    match ty {
        Type::Lumber => match run {
            Some("x") => Some(false),
            Some("y") => Some(true),
            None if two_point => Some(false),
            _ => None,
        },
        Type::Steel => match node.get("orient").and_then(Value::str) {
            Some("elevation" | "plan") => Some(run == Some("y")),
            _ => None,
        },
        Type::Ijoist => (run != Some("z")).then_some(false),
        _ => None,
    }
}

/// `from` / `to` of a lumber or steel member: its start and end points, length and angle. `Some(None)`: not a two-point
/// member; None: reported, the member cannot be placed.
pub fn two_point(scene: &Scene, id: &str, ty: Type, node: &Value) -> Option<Option<TwoPoint>> {
    let base = format!("components/{id}");
    let (fv, tv) = (node.get("from").filter(|v| !v.is_null()), node.get("to").filter(|v| !v.is_null()));
    if fv.is_none() && tv.is_none() {
        if node.get("align").is_some_and(|v| !v.is_null()) {
            err(
                scene,
                id,
                &format!("{base}/align"),
                format!("'{id}': align only applies to a member placed with from/to"),
                "remove align, or give \"from\" and \"to\"",
            );
            return None;
        }
        return Some(None);
    }
    let (Some(fv), Some(tv)) = (fv, tv) else {
        let missing = if fv.is_none() { "from" } else { "to" };
        err(
            scene,
            id,
            &format!("{base}/{missing}"),
            format!("'{id}' has one of from/to: a two-point member needs both (its start and its end)"),
            "\"from\": \"tie@top_center\", \"to\": \"chord@bottom_center\"",
        );
        return None;
    };
    if !matches!(ty, Type::Lumber | Type::Steel) {
        return Some(None);
    }
    for key in PLACE_KEYS {
        if node.get(key).is_some_and(|v| !v.is_null()) {
            err(
                scene,
                id,
                &format!("{base}/{key}"),
                format!(
                    "'{id}' is placed with from/to (its length and angle come from the two points): remove '{key}'"
                ),
                &op_text(id, obj1(key, Value::Null)),
            );
            return None;
        }
    }
    if ty == Type::Lumber && node.get("run").and_then(Value::str) == Some("z") {
        err(
            scene,
            id,
            &format!("{base}/run"),
            format!("'{id}': from/to place a member in the section plane; run z (end-on) has no length there"),
            "remove run (a two-point member runs along x of its own frame), or remove from/to",
        );
        return None;
    }
    if ty == Type::Steel && !matches!(node.get("orient").and_then(Value::str), Some("elevation" | "plan")) {
        err(
            scene,
            id,
            &format!("{base}/orient"),
            format!("'{id}': from/to need a steel member seen lengthwise: orient elevation or plan"),
            &op_text(id, obj1("orient", json::string("elevation"))),
        );
        return None;
    }
    let p0 = scene.point(fv, id, &format!("{base}/from"))?;
    let p1 = scene.point(tv, id, &format!("{base}/to"))?;
    let d = p1.sub(p0);
    let len = d.len();
    if len < 1e-3 {
        err(
            scene,
            id,
            &base,
            format!("'{id}': from and to are the same point ({}, {})", fmt_ft_in(p0.x), fmt_ft_in(p0.y)),
            "point from and to at the two members the brace or web connects",
        );
        return None;
    }
    let run_y = node.get("run").and_then(Value::str) == Some("y");
    let angle = math::atan2(d.y, d.x) - if run_y { core::f64::consts::FRAC_PI_2 } else { 0.0 };
    let align = match node.get("align").and_then(Value::str) {
        Some("top") => 1.0,
        Some("bottom") => 0.0,
        _ => 0.5,
    };
    Some(Some(TwoPoint {
        p0,
        len,
        angle,
        align,
        ends: [ref_target(fv), ref_target(tv)],
        names: [point_text(fv), point_text(tv)],
    }))
}

// ---- cut specs ---------------------------------------------------------------------------------------------------

#[derive(Clone)]
enum End {
    Square,
    Plumb,
    Level,
    Angle(f64),
    Line(V2, V2),
    Trim(Target),
}

impl End {
    fn name(&self) -> String {
        match self {
            End::Square => "square".into(),
            End::Plumb => "plumb".into(),
            End::Level => "level".into(),
            End::Angle(a) => format!("bevel {}deg", deg(*a)),
            End::Line(..) => "bevel to line".into(),
            End::Trim(t) => format!("trim to {}", t.comp),
        }
    }
}

struct Notch {
    top: bool,
    /// Some(true): at the start end, Some(false): at the end end, None: at `x`.
    corner: Option<bool>,
    x: f64,
    w: f64,
    d: f64,
}

struct Bird {
    k: usize,
    on: Option<String>,
    q: V2,
    a: V2,
    b: V2,
    up: V2,
    hdir: V2,
}

/// A member's cuts in its frame, ready to apply once its placement is known.
pub struct Plan {
    frame: Frame,
    vref: f64,
    angle: f64,
    ends: [(End, Option<usize>); 2],
    notches: Vec<Notch>,
    bird: Option<Bird>,
    two: bool,
}

impl Plan {
    /// The local point of a two-point member that lands on `from`: the start of its aligned line.
    pub fn start_point(&self) -> V2 {
        self.frame.local(V2::init(0.0, self.vref))
    }
}

fn len_field(scene: &Scene, id: &str, path: &str, e: &Value, key: &str, positive: bool) -> Result<Option<f64>, ()> {
    let Some(v) = e.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    match units::parse_length(v) {
        Some(x) if x > 0.0 || (!positive && x >= 0.0) => Ok(Some(x)),
        _ => {
            err(
                scene,
                id,
                &format!("{path}/{key}"),
                format!(
                    "'{id}' {key} must be a {} length in inches (got {})",
                    if positive { "positive" } else { "non-negative" },
                    model::kind_or_text(v)
                ),
                CUTS_FIX,
            );
            Err(())
        }
    }
}

fn comp_box(c: &Comp, inst: u32, part: Option<&str>) -> Option<Box> {
    let mut b = Box::default();
    for p in c.world.iter().filter(|p| {
        p.instance == inst && p.kind == Kind::Body && !p.loops.is_empty() && part.is_none_or(|pn| p.part == pn)
    }) {
        for l in &p.loops {
            b.add_box(geom::loop_box(l));
        }
    }
    (!b.is_empty()).then_some(b)
}

/// A placed component named by a cut (`on`, `to`): reported when unknown or not built.
fn named<'s, 'a>(scene: &'s Scene<'a>, id: &str, path: &str, what: &str, name: &str) -> Option<&'s Comp<'a>> {
    let base = name.split(['#', '.']).next().unwrap_or(name);
    let Some(c) = scene.find(base) else {
        let ids = scene.comp_ids();
        let hint = model::nearest(base, &ids).map_or(String::new(), |n| format!(" Did you mean '{n}'?"));
        scene.diags.add(
            Level::Error,
            "E_REF_UNKNOWN",
            Some(id),
            Some(path),
            format!("'{id}' {what}: no component '{base}'.{hint} Components: {}", scene::join_ids(&ids)),
        );
        return None;
    };
    if base == id {
        err(
            scene,
            id,
            path,
            format!("'{id}' {what}: a member cannot cut itself"),
            "name the member it bears on or meets",
        );
        return None;
    }
    if c.state != State::Ok {
        scene.diags.add(
            Level::Error,
            "E_REF_UNKNOWN",
            Some(id),
            Some(path),
            format!("'{id}' {what}: component '{base}' did not build (see its own errors)"),
        );
        return None;
    }
    Some(c)
}

fn target_of(name: &str) -> Target {
    let (head, part) = match name.split_once('.') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (name, None),
    };
    let (comp, inst) = match head.split_once('#') {
        Some((a, b)) => (a, b.parse::<u32>().unwrap_or(0)),
        None => (head, 0),
    };
    Target { comp: comp.to_string(), inst, part }
}

/// The bearing corner a birdmouth seat lands on when the member has no `at`: the top corner of `on` on the low (outer)
/// side. None when the member has no birdmouth on another member (or it is not placed: `prepare` reports it).
pub fn auto_seat(scene: &Scene, node: &Value, angle: f64, run_y: bool) -> Option<V2> {
    let on = node
        .get("cuts")?
        .arr()?
        .iter()
        .find(|e| e.get("type").and_then(Value::str) == Some("birdmouth"))?
        .get("on")?
        .str()?;
    let t = target_of(on);
    let c = scene.find(&t.comp).filter(|c| c.state == State::Ok)?;
    let b = comp_box(c, t.inst, t.part.as_deref())?;
    let u = rot(angle, if run_y { V2::init(0.0, 1.0) } else { V2::init(1.0, 0.0) });
    let uphill_right = (u.y > 0.0) == (u.x > 0.0);
    Some(V2::init(if uphill_right { b.x0 } else { b.x1 }, b.y1))
}

fn seat_corner(angle: f64, frame: &Frame, hdir: V2) -> &'static str {
    let w = rot(angle, frame.local(hdir).sub(frame.local(V2::init(0.0, 0.0))));
    if w.x > 0.0 { "top_left" } else { "top_right" }
}

/// Read and check a member's `cuts` (and the ends a two-point member trims to) in its frame, add the anchors that need
/// no placement (birdmouth seat corners, cut corners when both ends are known). `Some(None)`: nothing to cut; None:
/// reported, the member fails.
pub fn prepare(
    scene: &Scene,
    id: &str,
    node: &Value,
    built: &mut Built,
    angle: f64,
    run_y: Option<bool>,
    two: Option<&TwoPoint>,
) -> Option<Option<Plan>> {
    let base = format!("components/{id}/cuts");
    let cv = node.get("cuts").filter(|v| !v.is_null());
    if cv.is_none() && two.is_none() {
        return Some(None);
    }
    let Some(run_y) = run_y else {
        err(
            scene,
            id,
            &base,
            format!(
                "'{id}': cuts apply to a member seen lengthwise (lumber run x or y, steel orient elevation or plan); an end-on member has no ends in the section"
            ),
            "set \"run\": \"x\" (or \"y\"), or remove cuts",
        );
        return None;
    };
    if matches!(node.get("mirror"), Some(Value::Bool(true))) {
        err(
            scene,
            id,
            &format!("components/{id}/mirror"),
            format!("'{id}': mirror and cuts do not combine (the cuts are in the member's own frame)"),
            &op_text(id, obj1("mirror", Value::Null)),
        );
        return None;
    }
    let frame = Frame::of(built.box_, run_y);
    if frame.len <= 1e-6 || frame.h <= 1e-6 {
        return Some(None);
    }
    let vref = two.map_or(frame.h / 2.0, |t| t.align * frame.h);
    let up = frame.dir(rot(-angle, V2::init(0.0, 1.0)));
    let xw = frame.dir(rot(-angle, V2::init(1.0, 0.0)));
    let mut ends: [(End, Option<usize>); 2] = [(End::Square, None), (End::Square, None)];
    if let Some(t) = two {
        for (i, e) in t.ends.iter().enumerate() {
            if let Some(tg) = e {
                ends[i] = (End::Trim(tg.clone()), None);
            }
        }
    }
    let mut plan = Plan { frame, vref, angle, ends, notches: Vec::new(), bird: None, two: two.is_some() };
    let list: &[Value] = match cv {
        None => &[],
        Some(v) => match v.arr() {
            Some(a) => a,
            None => {
                err(
                    scene,
                    id,
                    &base,
                    format!("'{id}': cuts must be a list of cut objects (got {})", model::kind_or_text(v)),
                    CUTS_FIX,
                );
                return None;
            }
        },
    };
    let mut explicit: [Option<usize>; 2] = [None, None];
    let mut ok = true;
    for (k, e) in list.iter().enumerate() {
        let path = format!("{base}/{k}");
        if read_cut(scene, id, node, &path, k, e, &mut plan, &mut explicit, up, xw).is_err() {
            ok = false;
        }
    }
    if !ok {
        return None;
    }
    if let Some(b) = &plan.bird {
        for (name, p) in [("seat_outer", b.q), ("seat_inner", b.b), ("heel_bottom", b.a)] {
            built.anchors.push(NamedAnchor { name: name.into(), p: frame.local(p) });
        }
    }
    let intrinsic = |e: &End| matches!(e, End::Square | End::Plumb | End::Level | End::Angle(_));
    if intrinsic(&plan.ends[0].0) && intrinsic(&plan.ends[1].0) {
        let lines = [end_line(&plan, 0, None, up, xw), end_line(&plan, 1, None, up, xw)];
        if let [Some(l0), Some(l1)] = lines {
            corner_anchors(&plan, built, [l0, l1]);
        }
    }
    let mut info = String::new();
    if let Some(t) = two {
        info.push_str(&format!(" from {} to {}", t.names[0], t.names[1]));
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, (e, k)) in plan.ends.iter().enumerate() {
        if k.is_some() || matches!(e, End::Trim(_)) {
            parts.push(format!("{} {}", e.name(), ["start", "end"][i]));
        }
    }
    if let Some(b) = &plan.bird {
        let seat = b.b.sub(b.q).len();
        let heel = b.q.y / b.up.y;
        let hap = (frame.h - b.q.y) / b.up.y;
        let on = b.on.as_deref().map_or(String::new(), |o| format!(" on {o}"));
        parts.push(format!("birdmouth{on} seat {} heel {} (HAP {})", fmt_ft_in(seat), fmt_ft_in(heel), fmt_ft_in(hap)));
    }
    let n_notch = plan.notches.len() + notch_count_by(list);
    if n_notch > 0 {
        parts.push(format!("{n_notch} notch{}", if n_notch == 1 { "" } else { "es" }));
    }
    if !parts.is_empty() {
        info.push_str(&format!(" cuts: {}", parts.join(", ")));
    }
    built.info.push_str(&info);
    Some(Some(plan))
}

fn notch_count_by(list: &[Value]) -> usize {
    list.iter().filter(|e| e.get("type").and_then(Value::str) == Some("notch") && e.get("by").is_some()).count()
}

fn allowed(ty: &str) -> &'static [&'static str] {
    match ty {
        "square" | "plumb" | "level" => &["type", "end"],
        "bevel" => &["type", "end", "angle", "line"],
        "trim" => &["type", "end", "to"],
        "birdmouth" => &["type", "on", "seat", "heel", "tail"],
        _ => &["type", "by", "at", "w", "d", "x"],
    }
}

#[allow(clippy::too_many_arguments)]
fn read_cut(
    scene: &Scene,
    id: &str,
    node: &Value,
    path: &str,
    k: usize,
    e: &Value,
    plan: &mut Plan,
    explicit: &mut [Option<usize>; 2],
    up: V2,
    xw: V2,
) -> Result<(), ()> {
    let Value::Object(members) = e else {
        err(
            scene,
            id,
            path,
            format!("'{id}' cuts[{k}] must be an object with a \"type\" (got {})", model::kind_or_text(e)),
            CUTS_FIX,
        );
        return Err(());
    };
    let Some(ty) = e.get("type").and_then(Value::str).filter(|t| TYPES.contains(t)) else {
        err(
            scene,
            id,
            &format!("{path}/type"),
            format!("'{id}' cuts[{k}] needs a \"type\": one of {}", model::join_quoted(&TYPES)),
            CUTS_FIX,
        );
        return Err(());
    };
    let keys = allowed(ty);
    if let Some(m) = members.iter().find(|m| !keys.contains(&m.key.as_str())) {
        err(
            scene,
            id,
            &format!("{path}/{}", m.key),
            format!("'{id}' cuts[{k}] ({ty}): unknown key '{}'; a {ty} cut takes {}", m.key, keys.join(", ")),
            CUTS_FIX,
        );
        return Err(());
    }
    match ty {
        "birdmouth" => read_birdmouth(scene, id, node, path, k, e, plan, up, xw),
        "notch" => read_notch(scene, id, path, k, e, plan),
        _ => {
            let end = e.get("end").and_then(Value::str);
            let which: &[usize] = match end {
                Some("start") => &[0],
                Some("end") => &[1],
                Some("both") | None if matches!(ty, "square" | "plumb" | "level") || e.get("angle").is_some() => {
                    &[0, 1]
                }
                _ => {
                    err(
                        scene,
                        id,
                        &format!("{path}/end"),
                        format!(
                            "'{id}' cuts[{k}] ({ty}): end must be \"start\" or \"end\"{}",
                            if matches!(ty, "trim") || e.get("line").is_some() { "" } else { " or \"both\"" }
                        ),
                        CUTS_FIX,
                    );
                    return Err(());
                }
            };
            let kind = match ty {
                "square" => End::Square,
                "plumb" => End::Plumb,
                "level" => End::Level,
                "trim" => {
                    let Some(to) = e.get("to").and_then(Value::str) else {
                        err(
                            scene,
                            id,
                            &format!("{path}/to"),
                            format!("'{id}' cuts[{k}] (trim): \"to\" names the member whose face the end is cut to"),
                            "{\"type\": \"trim\", \"end\": \"end\", \"to\": \"top_chord\"}",
                        );
                        return Err(());
                    };
                    named(scene, id, &format!("{path}/to"), &format!("cuts[{k}] trim to"), to).ok_or(())?;
                    End::Trim(target_of(to))
                }
                _ => match (e.get("angle"), e.get("line")) {
                    (Some(a), None) => match a.num().filter(|a| a.abs() < 80.0) {
                        Some(a) => End::Angle(a * geom::RAD_PER_DEG),
                        None => {
                            err(
                                scene,
                                id,
                                &format!("{path}/angle"),
                                format!(
                                    "'{id}' cuts[{k}] (bevel): angle is degrees from square, between -80 and 80 (got {})",
                                    model::kind_or_text(a)
                                ),
                                "{\"type\": \"bevel\", \"end\": \"end\", \"angle\": 45}",
                            );
                            return Err(());
                        }
                    },
                    (None, Some(l)) => {
                        let pts = l.arr().filter(|a| a.len() == 2);
                        let Some(pts) = pts else {
                            err(
                                scene,
                                id,
                                &format!("{path}/line"),
                                format!(
                                    "'{id}' cuts[{k}] (bevel): line is two points [p, q] (Refs or [x, y]) the end is cut along"
                                ),
                                "{\"type\": \"bevel\", \"end\": \"end\", \"line\": [\"ridge@top_left\", \"ridge@bottom_left\"]}",
                            );
                            return Err(());
                        };
                        let p = scene.point(&pts[0], id, &format!("{path}/line/0")).ok_or(())?;
                        let q = scene.point(&pts[1], id, &format!("{path}/line/1")).ok_or(())?;
                        if p.dist(q) < 1e-6 {
                            err(
                                scene,
                                id,
                                &format!("{path}/line"),
                                format!("'{id}' cuts[{k}] (bevel): the two line points are the same"),
                                "give two distinct points on the line",
                            );
                            return Err(());
                        }
                        End::Line(p, q)
                    }
                    _ => {
                        err(
                            scene,
                            id,
                            path,
                            format!(
                                "'{id}' cuts[{k}] (bevel): give exactly one of angle (degrees from square) or line (two points)"
                            ),
                            "{\"type\": \"bevel\", \"end\": \"end\", \"angle\": 45}",
                        );
                        return Err(());
                    }
                },
            };
            for &i in which {
                if let Some(j) = explicit[i] {
                    err(
                        scene,
                        id,
                        path,
                        format!("'{id}': cuts[{j}] and cuts[{k}] both cut the {} end; keep one", ["start", "end"][i]),
                        &cuts_op(id, node, k, None),
                    );
                    return Err(());
                }
                explicit[i] = Some(k);
                plan.ends[i] = (kind.clone(), Some(k));
            }
            for &i in which {
                if end_line(plan, i, None, up, xw).is_none() && !matches!(kind, End::Line(..) | End::Trim(_)) {
                    err(
                        scene,
                        id,
                        path,
                        format!(
                            "'{id}' cuts[{k}] ({ty}): the cut runs along the member (it is {} degrees from horizontal), so it cannot close the end",
                            deg(math::atan2(up.x, up.y).abs())
                        ),
                        "use a square or level cut on a vertical member, plumb on a level one",
                    );
                    return Err(());
                }
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn read_birdmouth(
    scene: &Scene,
    id: &str,
    node: &Value,
    path: &str,
    k: usize,
    e: &Value,
    plan: &mut Plan,
    up: V2,
    xw: V2,
) -> Result<(), ()> {
    let f = plan.frame;
    if plan.bird.is_some() {
        err(
            scene,
            id,
            path,
            format!("'{id}' has two birdmouth cuts; a member bears on one seat"),
            &cuts_op(id, node, k, None),
        );
        return Err(());
    }
    if up.y < 0.087 || up.x.abs() < 0.0175 {
        err(
            scene,
            id,
            path,
            format!(
                "'{id}' cuts[{k}] (birdmouth): a birdmouth is cut in a sloped member whose bottom face looks down; '{id}' is at {} degrees{}",
                deg(math::atan2(up.x, up.y)),
                if up.y < 0.087 { " (its bottom face does not look down)" } else { " (level)" }
            ),
            "slope the member (\"slope\": \"6:12\") or use a notch: {\"type\": \"notch\", \"at\": \"bottom_left\", \"w\": 3.5, \"d\": 1.5}",
        );
        return Err(());
    }
    let low_start = up.x > 0.0;
    let hdir = if (xw.x > 0.0) == low_start { xw } else { xw.scale(-1.0) };
    let on = e.get("on").filter(|v| !v.is_null());
    let on_name = match on {
        None => None,
        Some(v) => match v.str() {
            Some(s) => {
                named(scene, id, &format!("{path}/on"), &format!("cuts[{k}] birdmouth on"), s).ok_or(())?;
                Some(s.to_string())
            }
            None => {
                err(
                    scene,
                    id,
                    &format!("{path}/on"),
                    format!(
                        "'{id}' cuts[{k}] (birdmouth): on names the member the seat bears on (got {})",
                        model::kind_or_text(v)
                    ),
                    "\"on\": \"top_plate\"",
                );
                return Err(());
            }
        },
    };
    let seat_in = len_field(scene, id, path, e, "seat", true)?;
    let heel_in = len_field(scene, id, path, e, "heel", true)?;
    let tail = len_field(scene, id, path, e, "tail", false)?.unwrap_or(0.0);
    let sv = -hdir.y;
    let seat = match (seat_in, heel_in) {
        (Some(_), Some(_)) => {
            err(
                scene,
                id,
                path,
                format!(
                    "'{id}' cuts[{k}] (birdmouth): give seat or heel, not both (one sets the other through the slope)"
                ),
                &cuts_op(id, node, k, Some(drop_key(e, "heel"))),
            );
            return Err(());
        }
        (Some(s), None) => s,
        (None, Some(hc)) => hc * up.y / sv,
        (None, None) => {
            let width = on_name.as_deref().and_then(|o| {
                let t = target_of(o);
                scene.find(&t.comp).and_then(|c| comp_box(c, t.inst, t.part.as_deref()))
            });
            match width {
                Some(b) => b.width(),
                None => {
                    err(
                        scene,
                        id,
                        path,
                        format!(
                            "'{id}' cuts[{k}] (birdmouth): give the seat length (or on: the member it bears on, whose width is the default seat)"
                        ),
                        "{\"type\": \"birdmouth\", \"on\": \"top_plate\"} or {\"type\": \"birdmouth\", \"seat\": 3.5}",
                    );
                    return Err(());
                }
            }
        }
    };
    let qv = seat * sv;
    let h = f.h;
    let max_seat = (h / 3.0) / sv;
    if qv >= h - 1e-6 {
        err(
            scene,
            id,
            &format!("{path}/seat"),
            format!(
                "'{id}' cuts[{k}] (birdmouth): a {} seat cuts {} into the {} member, through it",
                fmt_ft_in(seat),
                fmt_ft_in(qv),
                fmt_ft_in(h)
            ),
            &cuts_op(id, node, k, Some(set_key(e, "seat", sixteenth_floor(max_seat)))),
        );
        return Err(());
    }
    let ue = if low_start { 0.0 } else { f.len };
    let ev = V2::init(ue, plan.vref);
    let qu = (tail + ev.dot(hdir) - qv * hdir.y) / hdir.x;
    let q = V2::init(qu, qv);
    let a = q.sub(up.scale(qv / up.y));
    let b = q.add(hdir.scale(seat));
    let (lo, hi) = (q.x.min(a.x).min(b.x), q.x.max(a.x).max(b.x));
    if hi < -1e-6 || lo > f.len + 1e-6 || q.x.max(b.x) < 0.0 || q.x.min(b.x) > f.len {
        err(
            scene,
            id,
            &format!("{path}/tail"),
            format!(
                "'{id}' cuts[{k}] (birdmouth): with tail {} the seat lies {} along the member, outside its {} length",
                fmt_ft_in(tail),
                format_args!("{}..{}", fmt_ft_in(lo), fmt_ft_in(hi)),
                fmt_ft_in(f.len)
            ),
            "lengthen the member, or shorten tail (the horizontal distance from the plumb cut of the seat to the low end)",
        );
        return Err(());
    }
    if qv > h / 3.0 + 1e-6 {
        let pct = math::round(qv / h * 100.0);
        scene.diags.add_fix(
            Level::Warning,
            "W_BIRDMOUTH_DEPTH",
            Some(id),
            Some(path),
            format!(
                "'{id}' birdmouth cuts {} into the {} depth ({}%) with a {} seat; keep the cut within 1/3 of the depth ({}) so the rafter keeps its section at the bearing (common framing rule; IRC R802.7.1 limits notches in sawn lumber)",
                fmt_ft_in(qv),
                fmt_ft_in(h),
                model::fmt_num(pct),
                fmt_ft_in(seat),
                fmt_ft_in(h / 3.0)
            ),
            format!(
                "shorten the seat to {} (it then bears {} on the plate): {}; or use a deeper member",
                fmt_ft_in(sixteenth_floor(max_seat)),
                fmt_ft_in(sixteenth_floor(max_seat)),
                cuts_op(id, node, k, Some(set_key(&drop_key(e, "heel"), "seat", sixteenth_floor(max_seat))))
            ),
        );
    }
    plan.bird = Some(Bird { k, on: on_name, q, a, b, up, hdir });
    Ok(())
}

fn drop_key(e: &Value, key: &str) -> Value {
    match e {
        Value::Object(m) => Value::Object(m.iter().filter(|m| m.key != key).cloned().collect()),
        _ => e.clone(),
    }
}

fn set_key(e: &Value, key: &str, x: f64) -> Value {
    let mut m: Vec<Member> = match e {
        Value::Object(m) => m.clone(),
        _ => Vec::new(),
    };
    match m.iter_mut().find(|m| m.key == key) {
        Some(mm) => mm.value = Value::Number(x),
        None => m.push(Member { key: key.into(), value: Value::Number(x) }),
    }
    Value::Object(m)
}

fn read_notch(scene: &Scene, id: &str, path: &str, k: usize, e: &Value, plan: &mut Plan) -> Result<(), ()> {
    let f = plan.frame;
    if let Some(by) = e.get("by") {
        if let Some(other) = ["at", "w", "d", "x"].into_iter().find(|key| e.get(key).is_some()) {
            err(
                scene,
                id,
                &format!("{path}/{other}"),
                format!("'{id}' cuts[{k}] (notch): by (subtract a member) and a rectangle (at, w, d) do not combine"),
                CUTS_FIX,
            );
            return Err(());
        }
        if by.str().is_none() {
            err(
                scene,
                id,
                &format!("{path}/by"),
                format!(
                    "'{id}' cuts[{k}] (notch): by names the member whose outline is cut out (got {})",
                    model::kind_or_text(by)
                ),
                "{\"type\": \"notch\", \"by\": \"top_plate\"}",
            );
            return Err(());
        }
        return Ok(());
    }
    let at = e.get("at").and_then(Value::str);
    let corners = ["top_left", "top_right", "bottom_left", "bottom_right", "top", "bottom"];
    let Some(at) = at.filter(|a| corners.contains(a)) else {
        err(
            scene,
            id,
            &format!("{path}/at"),
            format!(
                "'{id}' cuts[{k}] (notch): give by (a member) or at: one of {} with w (along the member) and d (into it)",
                corners.join(", ")
            ),
            "{\"type\": \"notch\", \"at\": \"top_right\", \"w\": 3.5, \"d\": 1.5}",
        );
        return Err(());
    };
    let need = |key: &str| -> Result<f64, ()> {
        match len_field(scene, id, path, e, key, true)? {
            Some(x) => Ok(x),
            None => {
                err(
                    scene,
                    id,
                    &format!("{path}/{key}"),
                    format!(
                        "'{id}' cuts[{k}] (notch): {key} is required ({})",
                        if key == "w" { "length along the member" } else { "depth into the member" }
                    ),
                    "{\"type\": \"notch\", \"at\": \"top_right\", \"w\": 3.5, \"d\": 1.5}",
                );
                Err(())
            }
        }
    };
    let w = need("w")?;
    let d = need("d")?;
    let top = at.starts_with("top");
    let corner = match at {
        "top_left" | "bottom_left" => Some(true),
        "top_right" | "bottom_right" => Some(false),
        _ => None,
    };
    let x = match corner {
        Some(_) => {
            if e.get("x").is_some() {
                err(
                    scene,
                    id,
                    &format!("{path}/x"),
                    format!(
                        "'{id}' cuts[{k}] (notch): x places a notch at \"top\"/\"bottom\"; a corner notch starts at its end"
                    ),
                    CUTS_FIX,
                );
                return Err(());
            }
            0.0
        }
        None => match len_field(scene, id, path, e, "x", false)? {
            Some(x) => x,
            None => {
                err(
                    scene,
                    id,
                    &format!("{path}/x"),
                    format!(
                        "'{id}' cuts[{k}] (notch at {at}): x (distance from the start end to the notch) is required"
                    ),
                    "{\"type\": \"notch\", \"at\": \"top\", \"x\": 24, \"w\": 3.5, \"d\": 1.5}",
                );
                return Err(());
            }
        },
    };
    if d >= f.h - 1e-6 || w > f.len + 1e-6 || x + w > f.len + 1e-6 {
        err(
            scene,
            id,
            path,
            format!(
                "'{id}' cuts[{k}] (notch): a {} x {} notch{} does not fit the member ({} long, {} deep)",
                fmt_ft_in(w),
                fmt_ft_in(d),
                if corner.is_none() { format!(" at x {}", fmt_ft_in(x)) } else { String::new() },
                fmt_ft_in(f.len),
                fmt_ft_in(f.h)
            ),
            "make the notch shallower than the member and within its length",
        );
        return Err(());
    }
    plan.notches.push(Notch { top, corner, x, w, d });
    Ok(())
}

// ---- applying ----------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Line {
    p: V2,
    t: V2,
}

impl Line {
    /// u where the line crosses v = `v` (None when it runs along the member).
    fn u_at(&self, v: f64) -> Option<f64> {
        (self.t.y.abs() > 1e-9 * self.t.len()).then(|| self.p.x + (v - self.p.y) * self.t.x / self.t.y)
    }
}

/// The line of end `i` in frame coordinates; `world` maps world points into the builder's local coordinates (needed
/// by bevel-to-line and trim; None in the placement-free pass, where they are not resolved).
fn end_line(plan: &Plan, i: usize, world: Option<(&Scene, &Xf)>, up: V2, xw: V2) -> Option<Line> {
    let u_end = if i == 0 { 0.0 } else { plan.frame.len };
    let p = V2::init(u_end, plan.vref);
    let t = match &plan.ends[i].0 {
        End::Square => V2::init(0.0, 1.0),
        End::Plumb => up,
        End::Level => xw,
        End::Angle(a) => V2::init(-math::sin(*a), math::cos(*a)),
        End::Line(a, b) => {
            let (_, inv) = world?;
            let to = plan.frame.frame_xf();
            let pa = to.apply(inv.apply(*a));
            let pb = to.apply(inv.apply(*b));
            let l = Line { p: pa, t: pb.sub(pa) };
            return l.u_at(0.0).map(|_| l);
        }
        End::Trim(_) => return None,
    };
    let l = Line { p, t };
    l.u_at(0.0).and(l.u_at(plan.frame.h)).map(|_| l)
}

/// The face of `tg` that end `i` of the member meets along its reference line, in frame coordinates. None: the line
/// misses it.
fn trim_line(scene: &Scene, plan: &Plan, i: usize, tg: &Target, inv: &Xf) -> Option<Line> {
    let c = scene.find(&tg.comp)?;
    let to = plan.frame.frame_xf();
    let f = plan.frame;
    let mid = f.len / 2.0;
    let dir = if i == 0 { -1.0 } else { 1.0 };
    let reach = mid + f.len.max(48.0);
    let mut best: Option<(f64, f64, Line)> = None;
    for p in c.world.iter().filter(|p| {
        p.instance == tg.inst
            && (p.kind == Kind::Body || p.dashed)
            && !p.loops.is_empty()
            && tg.part.as_deref().is_none_or(|pn| p.part == pn)
    }) {
        for lp in &p.loops {
            let pts: Vec<V2> = geom::flatten_loop(lp, FLAT_TOL).into_iter().map(|q| to.apply(inv.apply(q))).collect();
            for (j, &a) in pts.iter().enumerate() {
                let b = pts[(j + 1) % pts.len()];
                let dv = b.y - a.y;
                if dv.abs() < 1e-12 {
                    continue;
                }
                let s = (plan.vref - a.y) / dv;
                if !(-1e-9..=1.0 + 1e-9).contains(&s) {
                    continue;
                }
                let u = a.x + s * (b.x - a.x);
                let dist = (u - mid) * dir;
                if dist <= 1e-9 || dist > reach {
                    continue;
                }
                let head_on = dv.abs() / b.dist(a);
                let better = match &best {
                    None => true,
                    Some((bd, bh, _)) => dist < bd - 1e-7 || ((dist - bd).abs() <= 1e-7 && head_on > *bh),
                };
                if better {
                    best = Some((dist, head_on, Line { p: V2::init(u, plan.vref), t: b.sub(a) }));
                }
            }
        }
    }
    best.map(|b| b.2)
}

fn corner_anchors(plan: &Plan, built: &mut Built, lines: [Line; 2]) {
    let h = plan.frame.h;
    let (Some(s0), Some(sh), Some(e0), Some(eh)) =
        (lines[0].u_at(0.0), lines[0].u_at(h), lines[1].u_at(0.0), lines[1].u_at(h))
    else {
        return;
    };
    for (name, p) in [
        ("start_bottom", V2::init(s0, 0.0)),
        ("start_top", V2::init(sh, h)),
        ("end_bottom", V2::init(e0, 0.0)),
        ("end_top", V2::init(eh, h)),
    ] {
        if !built.anchors.iter().any(|a| a.name == name) {
            built.anchors.push(NamedAnchor { name: name.into(), p: plan.frame.local(p) });
        }
    }
}

/// The pieces of segment a-b inside `region` (even-odd over its loops).
fn clip_segment(a: V2, b: V2, region: &[Vec<V2>]) -> Vec<[V2; 2]> {
    let mut ts = vec![0.0, 1.0];
    for l in region {
        for (j, &p) in l.iter().enumerate() {
            let q = l[(j + 1) % l.len()];
            let mut ta = [0.0; 2];
            let mut tb = [0.0; 2];
            let n = geom::seg_seg(a, b, p, q, &mut ta, &mut tb);
            ts.extend(ta.iter().take(n).copied().filter(|t| (0.0..=1.0).contains(t)));
        }
    }
    crate::sort::stable(&mut ts, |x, y| x < y);
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    let mut out: Vec<[V2; 2]> = Vec::new();
    for w in ts.windows(2) {
        let m = a.lerp(b, (w[0] + w[1]) / 2.0);
        if geom::locate_even_odd(m, region, 0.0) != geom::Loc::Outside {
            let (p, q) = (a.lerp(b, w[0]), a.lerp(b, w[1]));
            match out.last_mut() {
                Some(last) if last[1].eql(p, 1e-9) => last[1] = q,
                _ => out.push([p, q]),
            }
        }
    }
    out
}

/// Cut the member: resolve the ends that need its placement (`xf`: local -> world of instance 0), stretch its prisms to
/// reach both end lines, clip them to the strip between the lines minus the notches, add the cut-corner anchors and
/// check the birdmouth seat on its bearing member. False: reported, the member fails.
pub fn apply(scene: &Scene, id: &str, node: &Value, plan: &Plan, built: &mut Built, xf: &Xf) -> bool {
    let f = plan.frame;
    let base = format!("components/{id}/cuts");
    let inv = inverse(xf);
    let up = f.dir(rot(-plan.angle, V2::init(0.0, 1.0)));
    let xw = f.dir(rot(-plan.angle, V2::init(1.0, 0.0)));
    let mut lines: [Line; 2] = [Line { p: V2::init(0.0, plan.vref), t: V2::init(0.0, 1.0) }; 2];
    for i in 0..2 {
        let (e, k) = &plan.ends[i];
        let path = k.map_or(format!("components/{id}/{}", ["from", "to"][i]), |k| format!("{base}/{k}"));
        let resolved = match e {
            End::Trim(tg) => trim_line(scene, plan, i, tg, &inv),
            _ => end_line(plan, i, Some((scene, &inv)), up, xw),
        };
        match resolved {
            Some(l) if l.u_at(0.0).is_some() && l.u_at(f.h).is_some() => lines[i] = l,
            _ => {
                let what = ["start", "end"][i];
                let (msg, fix) = match e {
                    End::Trim(tg) => (
                        format!(
                            "the {what} of '{id}' does not meet '{}': its {} line misses it (the end is left square)",
                            tg.comp,
                            if plan.two && plan.vref != f.h / 2.0 { "aligned" } else { "center" }
                        ),
                        match k {
                            Some(k) => format!(
                                "move '{id}' so its {what} reaches '{}', or drop the cut: {}",
                                tg.comp,
                                cuts_op(id, node, *k, None)
                            ),
                            None => format!(
                                "point {} at an anchor on the face of '{}' that '{id}' meets (e.g. \"{}@bottom_center\")",
                                ["from", "to"][i],
                                tg.comp,
                                tg.comp
                            ),
                        },
                    ),
                    _ => (
                        format!(
                            "the {what} cut of '{id}' ({}) runs along the member and cannot close it (the end is left square)",
                            e.name()
                        ),
                        match k {
                            Some(k) => format!("drop or change the cut: {}", cuts_op(id, node, *k, None)),
                            None => "change the cut".to_string(),
                        },
                    ),
                };
                scene.diags.add_fix(Level::Warning, "W_CUT_MISS", Some(id), Some(&path), msg, fix);
                let u_end = if i == 0 { 0.0 } else { f.len };
                lines[i] = Line { p: V2::init(u_end, plan.vref), t: V2::init(0.0, 1.0) };
            }
        }
    }
    let (s0, sh, e0, eh) = (
        lines[0].u_at(0.0).unwrap_or(0.0),
        lines[0].u_at(f.h).unwrap_or(0.0),
        lines[1].u_at(0.0).unwrap_or(f.len),
        lines[1].u_at(f.h).unwrap_or(f.len),
    );
    let bound = 10.0 * (f.len + f.h) + 100.0;
    if [s0, sh, e0, eh].iter().any(|u| u.abs() > bound) {
        err(
            scene,
            id,
            &base,
            format!(
                "'{id}': an end cut ({} start, {} end) runs almost along the member and meets its faces {} or more from its ends",
                plan.ends[0].0.name(),
                plan.ends[1].0.name(),
                fmt_ft_in(bound - f.len)
            ),
            "cut the end square, plumb or level instead, or move the member its end is cut to",
        );
        return false;
    }
    if s0 > e0 - 1e-3 || sh > eh - 1e-3 {
        err(
            scene,
            id,
            &base,
            format!(
                "'{id}': its end cuts ({} start, {} end) cross inside the member: along the bottom face they are at {} and {}, along the top at {} and {} of its {} length",
                plan.ends[0].0.name(),
                plan.ends[1].0.name(),
                fmt_ft_in(s0),
                fmt_ft_in(e0),
                fmt_ft_in(sh),
                fmt_ft_in(eh),
                fmt_ft_in(f.len)
            ),
            "lengthen the member, or move the members its ends are cut to",
        );
        return false;
    }
    let lo = s0.min(sh).min(0.0) - 0.5;
    let hi = e0.max(eh).max(f.len) + 0.5;
    let stretch = Xf::translate(lo, 0.0).mul(Xf::scaling((hi - lo) / f.len, 1.0));
    let sx = f.local_xf().mul(stretch).mul(f.frame_xf());
    let mut strip = vec![V2::init(s0, 0.0), V2::init(e0, 0.0), V2::init(eh, f.h), V2::init(sh, f.h)];
    clip::orient_ccw(&mut strip);
    let mut region: Vec<Vec<V2>> = vec![strip];
    let (vlo, vhi) = (-1.0, f.h + 1.0);
    let mut notch_polys: Vec<Vec<V2>> = Vec::new();
    for n in &plan.notches {
        let (u0, u1) = match n.corner {
            Some(true) => (lo - 1.0, n.w),
            Some(false) => (f.len - n.w, hi + 1.0),
            None => (n.x, n.x + n.w),
        };
        let (v0, v1) = if n.top { (f.h - n.d, vhi) } else { (vlo, n.d) };
        notch_polys.push(vec![V2::init(u0, v0), V2::init(u1, v0), V2::init(u1, v1), V2::init(u0, v1)]);
    }
    if let Some(b) = &plan.bird {
        let reach = b.q.y + 1.0;
        let a = b.q.sub(b.up.scale(reach / b.up.y));
        let c = b.q.add(b.hdir.scale(reach / -b.hdir.y));
        let mut tri = vec![b.q, a, c];
        clip::orient_ccw(&mut tri);
        notch_polys.push(tri);
    }
    for np in &notch_polys {
        region = clip::boolean(&region, core::slice::from_ref(np), Op::Diff);
    }
    let to_local = f.local_xf();
    let region_local: Vec<Vec<V2>> = region.iter().map(|l| l.iter().map(|&p| to_local.apply(p)).collect()).collect();
    let mut out: Vec<Prism> = Vec::with_capacity(built.prisms.len());
    for p in &built.prisms {
        let q = p.transform(&sx);
        if q.loops.is_empty() {
            let pts: Vec<V2> = q.line_pts.iter().map(|p| p.v()).collect();
            for w in pts.windows(2) {
                for s in clip_segment(w[0], w[1], &region_local) {
                    let mut r = q.clone();
                    r.line_pts = vec![Pt::at(s[0], 0.0), Pt::at(s[1], 0.0)];
                    out.push(r);
                }
            }
            continue;
        }
        let flat = clip::from_region(&q.loops, FLAT_TOL);
        let inter = clip::boolean(&flat, &region_local, Op::Intersect);
        if clip::total_area(&inter) <= 1e-9 {
            continue;
        }
        for g in group_region(&inter, None) {
            let mut r = q.clone();
            r.ply_lines = q.ply_lines.iter().flat_map(|s| clip_segment(s[0], s[1], &g.flat)).collect();
            r.loops = g.loops;
            out.push(r);
        }
    }
    built.prisms = out;
    corner_anchors(plan, built, lines);
    if let Some(b) = &plan.bird
        && let Some(on) = &b.on
    {
        check_seat(scene, id, plan, b, on, xf);
    }
    true
}

fn check_seat(scene: &Scene, id: &str, plan: &Plan, b: &Bird, on: &str, xf: &Xf) {
    let f = plan.frame;
    let t = target_of(on);
    let Some(c) = scene.find(&t.comp) else { return };
    let Some(ob) = comp_box(c, t.inst, t.part.as_deref()) else { return };
    let q = xf.apply(f.local(b.q));
    let s = xf.apply(f.local(b.b));
    let (x0, x1) = (q.x.min(s.x), q.x.max(s.x));
    let level = (q.y - ob.y1).abs() <= SEAT_TOL;
    let over = x0 >= ob.x0 - SEAT_TOL && x1 <= ob.x1 + SEAT_TOL;
    if level && over {
        return;
    }
    let why = if !level {
        format!(
            "the seat is {} {} its top",
            fmt_ft_in((q.y - ob.y1).abs()),
            if q.y > ob.y1 { "above" } else { "below" }
        )
    } else {
        format!(
            "the seat runs x {}..{} past its x {}..{}",
            fmt_ft_in(x0),
            fmt_ft_in(x1),
            fmt_ft_in(ob.x0),
            fmt_ft_in(ob.x1)
        )
    };
    let corner = seat_corner(plan.angle, &f, b.hdir);
    let at = obj1(
        "at",
        Value::Object(vec![
            Member { key: "anchor".into(), value: json::string("seat_outer") },
            Member { key: "to".into(), value: json::string(&format!("{on}@{corner}")) },
        ]),
    );
    scene.diags.add_fix(
        Level::Warning,
        "W_CUT_MISS",
        Some(id),
        Some(&format!("components/{id}/cuts/{}", b.k)),
        format!("the birdmouth seat of '{id}' does not bear on '{on}': {why}"),
        format!(
            "place the seat on it (or remove \"at\": a birdmouth with on places the member itself): {}",
            op_text(id, at)
        ),
    );
}

/// The fix for an overlap (`rise` high) of a member with a birdmouth seat and `other`, when `other` is not its bearing
/// member: the heel (plumb cut) is so deep that the member reaches below its bearing member's top into `other`. A
/// shallower heel lifts the member by the difference, so the exact fix is the heel minus the overlap height (and no
/// deeper than the bearing member).
pub fn birdmouth_overlap_fix(m: &Comp, other: &Comp, comps: &[&Comp], rise: f64) -> Option<String> {
    let list = m.node.get("cuts")?.arr()?;
    let (k, e) = list.iter().enumerate().find(|(_, e)| e.get("type").and_then(Value::str) == Some("birdmouth"))?;
    let on = e.get("on")?.str()?;
    let t = target_of(on);
    if t.comp == other.id {
        return None;
    }
    let anchor = |n: &str| m.built.anchors.iter().find(|a| a.name == n).map(|a| a.p);
    let heel = anchor("seat_outer")?.dist(anchor("heel_bottom")?);
    let c = comps.iter().find(|c| c.id == t.comp)?;
    let depth = comp_box(c, t.inst, t.part.as_deref())?.height();
    let new = sixteenth_floor((heel - rise).min(depth));
    if new <= 0.0 {
        return None;
    }
    let entry = set_key(&drop_key(&drop_key(e, "seat"), "heel"), "heel", new);
    Some(format!(
        "the birdmouth heel of '{}' ({}) drops it below the top of '{on}' into '{}': a {} heel (plumb cut) lifts it clear: {}",
        m.id,
        fmt_ft_in(heel),
        other.id,
        fmt_ft_in(new),
        cuts_op(&m.id, m.node, k, Some(entry))
    ))
}
