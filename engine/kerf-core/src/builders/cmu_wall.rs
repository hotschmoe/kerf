//! cmu wall (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, fmt_num, parse_cover, zone_rect};
use crate::geom::{self, Box, V2};
use crate::json::Value;
use crate::model::{self, Built, HostInfo, Kind, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use crate::pen::Pen;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

param_enum! { pub enum Grout { Solid = "solid", Reinforced = "reinforced", None = "none" } }
param_enum! { pub enum Run { Z = "z", X = "x" } }
param_enum! { pub enum Bond { Running = "running", Stack = "stack" } }

params! {
    pub struct Params {
        width: Num = 8.0 => { desc: "nominal 6, 8, 10, 12 => actual 5.625, 7.625, 9.625, 11.625" },
        courses: Int(u8, 1, 200) => { desc: "number of 8\" courses (7.625 unit + 0.375 mortar joint)" },
        bond_beam_courses: Int(u8, 0, 200) = 0 => { desc: "top N courses are bond-beam units (always grouted)" },
        bond_beams: OptRaw => { def: "null", desc: "bond-beam courses anywhere in the wall (1 = bottom; always grouted): course numbers or [from, to] ranges, e.g. [4] (bond beam at a slab or ledger) or [[4, 5], 9]. Each run of adjacent bond-beam courses is a part bond_beam_<k> (k = 1 from the bottom) with anchor bond_beam_<k>_center; bond_beam is the highest. Adds to bond_beam_courses" },
        grout: Enum(Grout) = Grout::Reinforced => { desc: "solid | reinforced (bond beams + the cut cell) | none" },
        grout_courses: OptRaw => { def: "null", desc: "courses grouted solid when the rest are not (partial grouting), as bond_beams, e.g. [[1, 3]] below grade with grout \"none\"" },
        face_shell: LenPos = 1.25 => { desc: "face shell thickness drawn in section" },
        top_joint: Bool = false => { desc: "mortar joint above the top course" },
        run: Enum(Run) = Run::Z => { desc: "z: the wall runs toward the viewer (drawn in section); x: the wall runs along x with its face toward the viewer (face-on elevation: courses, head joints, control joints; needs length; thickness along z centred on z)" },
        length: OptLenPos => { def: "run x: required", desc: "run x: wall length along x" },
        control_joints: OptRaw => { def: "null", desc: "run x: control joints at these distances from the wall's left end, e.g. [96, 192]: a 3/8\" vertical joint through every course, the bond restarts in each panel" },
        bond: Enum(Bond) = Bond::Running => { desc: "run x: running (alternate courses offset half a unit) | stack" },
        cover: OptRaw => { def: "{sides:1.5, top:1.5, bottom:0.5}", desc: "required clear cover for rebar (W_COVER); supports cover.parts.<part> overrides" },
        voids: OptRaw => { def: "null", desc: "what the host loses (SPEC 26.4): component ids (\"column\", \"plate.part\": its outline over its z extent, so the hole follows the member) and shapes {\"rect\": [w, h], \"at\": Ref, \"anchor\": \"center\"} | {\"circle\": d, \"at\": Ref} | {\"points\": [...]}, each with optional \"z\": [z0, z1] (default: through). Cut in section, iso and 3D; embedded steel then shows with its own fill. W_COVER measures to the void edge" },
    }
}

const COURSE: f64 = 8.0;
const UNIT_H: f64 = 7.625;
const JOINT: f64 = 0.375;

/// The courses a list param names (`[4, [6, 8]]`: 4, 6, 7 and 8), as flags indexed by course - 1. None after an E_PARAM.
fn course_flags(ctx: &mut Ctx, key: &str, v: Option<&Value>, n: usize) -> Option<Vec<bool>> {
    let mut flags = alloc::vec![false; n];
    let Some(v) = v else { return Some(flags) };
    let example = format!("e.g. [4] or [[1, 3], {n}]");
    let Some(items) = v.arr() else {
        ctx.p.fail(
            key,
            format!(
                "param '{key}' must be a list of course numbers (1 = bottom) or [from, to] ranges, {example} (got {})",
                model::kind_or_text(v)
            ),
        );
        return None;
    };
    let course = |e: &Value| e.num().filter(|x| *x == crate::math::round(*x) && *x >= 1.0 && *x <= n as f64);
    for (i, e) in items.iter().enumerate() {
        let range = match e {
            Value::Number(_) => course(e).map(|k| [k, k]),
            Value::Array(r) if r.len() == 2 => course(&r[0]).zip(course(&r[1])).map(|(a, b)| [a, b]),
            _ => None,
        };
        let Some([a, b]) = range.filter(|r| r[0] <= r[1]) else {
            ctx.p.fail(
                &format!("{key}/{i}"),
                format!("{key}[{i}] must be a course from 1 to {n} (the wall has {n} courses, 1 = bottom) or a [from, to] range with from <= to, {example} (got {})", model::kind_or_text(e)),
            );
            return None;
        };
        for k in (a as usize)..=(b as usize) {
            flags[k - 1] = true;
        }
    }
    Some(flags)
}

/// Runs of adjacent flagged courses as (first, last), 1-based, bottom up.
fn runs(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, &f) in flags.iter().enumerate() {
        if !f {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.1 == i => last.1 = i + 1,
            _ => out.push((i + 1, i + 1)),
        }
    }
    out
}

fn runs_text(rs: &[(usize, usize)]) -> String {
    let parts: Vec<String> =
        rs.iter().map(|&(a, b)| if a == b { format!("{a}") } else { format!("{a}-{b}") }).collect();
    parts.join(", ")
}

/// Bottom and top y of courses a..=b (1-based).
fn course_span(a: usize, b: usize) -> (f64, f64) {
    ((a - 1) as f64 * COURSE, (b - 1) as f64 * COURSE + UNIT_H)
}

fn actual_width(ctx: &mut Ctx, x: f64) -> Option<f64> {
    for (nom, act) in [(6.0, 5.625), (8.0, 7.625), (10.0, 9.625), (12.0, 11.625)] {
        if x == nom || (x - act).abs() < 1e-3 {
            return Some(act);
        }
    }
    ctx.p.fail(
        "width",
        format!(
            "param 'width' must be a nominal 6, 8, 10 or 12 (actual 5.625, 7.625, 9.625, 11.625) (got {})",
            fmt_num(x)
        ),
    );
    None
}

/// What both runs share once the parameters are checked.
struct Wall {
    w: f64,
    fs: f64,
    n: usize,
    total_h: f64,
    bb: Vec<bool>,
    grouted: Vec<bool>,
    info: String,
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let cp = ctx.p.parse_all::<Params>();
    let cov = parse_cover(ctx, "cover", model::Cover { bottom: 0.5, sides: 1.5, top: 1.5 });
    if !ctx.p.ok {
        return None;
    }
    let cov = cov?;
    let w = actual_width(ctx, cp.width)?;
    let n = usize::from(cp.courses);
    let nbb = usize::from(cp.bond_beam_courses);
    if nbb > n {
        ctx.p.fail("bond_beam_courses", format!("param 'bond_beam_courses' ({nbb}) cannot exceed 'courses' ({n})"));
        return None;
    }
    if cp.face_shell * 2.0 >= w {
        ctx.p.fail(
            "face_shell",
            format!("face_shell {} leaves no cell in a {} wide wall", fmt_num(cp.face_shell), fmt_num(w)),
        );
        return None;
    }
    let mut bb = course_flags(ctx, "bond_beams", cp.bond_beams.as_ref(), n)?;
    for f in bb.iter_mut().skip(n - nbb) {
        *f = true;
    }
    let gc = course_flags(ctx, "grout_courses", cp.grout_courses.as_ref(), n)?;
    if cp.run == Run::Z {
        for key in ["length", "control_joints"] {
            if ctx.p.has(key) {
                ctx.p.fail(key, format!("param '{key}' is for a wall seen face-on: add \"run\": \"x\" (the wall runs along x), or remove '{key}'"));
                return None;
            }
        }
    }
    let all = cp.grout == Grout::Solid || (cp.grout == Grout::Reinforced && cp.run == Run::Z);
    if all && gc.iter().any(|&g| g) {
        ctx.p.diags.add_fix(
            model::Level::Warning,
            "W_PARAM",
            Some(ctx.p.id),
            Some(&format!("{}/{}/grout_courses", ctx.p.base, ctx.p.id)),
            format!("'{}': grout_courses has no effect with grout \"{}\": every course is already grouted", ctx.p.id, cp.grout.tag()),
            format!("set \"grout\": \"none\" so only the listed courses (and bond beams) are grouted: {{\"op\": \"update\", \"path\": \"components/{}\", \"value\": {{\"grout\": \"none\"}}}}", ctx.p.id),
        );
    }
    let grouted: Vec<bool> = (0..n).map(|i| all || bb[i] || gc[i]).collect();
    let total_h = n as f64 * COURSE - JOINT + if cp.top_joint { JOINT } else { 0.0 };
    let nom =
        [(5.625, 6.0), (7.625, 8.0), (9.625, 10.0)].iter().find(|(a, _)| (w - a).abs() < 1e-3).map_or(12.0, |t| t.1);
    let bb_runs = runs(&bb);
    let nbb_all = bb.iter().filter(|&&b| b).count();
    let top_only = bb_runs.len() == 1 && bb_runs[0].1 == n;
    let mut info = if cp.run == Run::X {
        format!(
            "cmu_wall {}\" x {n} courses run x L={}",
            fmt_num(nom),
            crate::units::fmt_ft_in(cp.length.unwrap_or(0.0))
        )
    } else {
        format!("cmu_wall {}\" x {n} courses", fmt_num(nom))
    };
    if top_only {
        info.push_str(&format!(" ({nbb_all} bond beam)"));
    } else if nbb_all > 0 {
        info.push_str(&format!(" (bond beams at courses {})", runs_text(&bb_runs)));
    }
    if !all && gc.iter().any(|&g| g) {
        info.push_str(&format!(", grouted courses {}", runs_text(&runs(&gc))));
    }
    let wall = Wall { w, fs: cp.face_shell, n, total_h, bb, grouted, info };
    let mut built = match cp.run {
        Run::Z => run_z(&wall, cp.top_joint),
        Run::X => run_x(ctx, &cp, &wall)?,
    };
    let outline = model::rect_loop(0.0, 0.0, built.box_.x1, total_h);
    built.host = Some(HostInfo { outline, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set });
    Some(built)
}

/// The bond-beam zones and anchors: `bond_beam_<k>` per run of adjacent courses, `bond_beam` = the highest.
fn bond_beam_zones(wall: &Wall, x0: f64, x1: f64, zones: &mut Vec<Zone>, anchors: &mut Vec<NamedAnchor>) -> V2 {
    let mut center = V2::init((x0 + x1) / 2.0, wall.total_h);
    let rs = runs(&wall.bb);
    for (k, &(a, b)) in rs.iter().enumerate() {
        let (y0, y1) = course_span(a, b);
        zones.push(zone_rect(&format!("bond_beam_{}", k + 1), x0, y0, x1, y1));
        center = V2::init((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        anchors.push(NamedAnchor { name: format!("bond_beam_{}_center", k + 1), p: center });
    }
    if let Some(&(a, b)) = rs.last() {
        let (y0, y1) = course_span(a, b);
        zones.push(zone_rect("bond_beam", x0, y0, x1, y1));
    }
    center
}

/// The `grout` zone: one rectangle per run of grouted courses between `x0` and `x1`.
fn grout_zone(wall: &Wall, x0: f64, x1: f64) -> Option<Zone> {
    let rs = runs(&wall.grouted);
    if rs.is_empty() {
        return None;
    }
    let mut loops = Vec::new();
    let mut bx = Box::default();
    for &(a, b) in &rs {
        let (y0, y1) = course_span(a, b);
        loops.push(model::rect_loop(x0, y0, x1, y1));
        bx.add_point(x0, y0);
        bx.add_point(x1, y1);
    }
    Some(Zone { name: "grout".into(), loops, box_: bx })
}

/// The wall in section (run z): per course two face shells, the cell (grouted or a beyond web line) and a bed joint.
fn run_z(wall: &Wall, top_joint: bool) -> Built {
    let (w, fs, n, total_h) = (wall.w, wall.fs, wall.n, wall.total_h);
    let mut prisms: Vec<Prism> = Vec::new();
    let mut zones = Vec::new();
    for k in 1..=n {
        let (y0, y1) = course_span(k, k);
        let part = format!("course_{k}");
        let unit = |loop_: Vec<geom::Pt>, part: &str, material: &str| Prism {
            part: part.into(),
            material: material.into(),
            loops: model::one_loop(loop_),
            cmu_unit: true,
            course: k as u32,
            ..Prism::default()
        };
        prisms.push(unit(model::rect_loop(0.0, y0, fs, y1), &part, "cmu"));
        prisms.push(unit(model::rect_loop(w - fs, y0, w, y1), &part, "cmu"));
        let cell = model::rect_loop(fs, y0, w - fs, y1);
        if wall.grouted[k - 1] {
            prisms.push(unit(cell, "grout", "grout"));
        } else {
            prisms.push(Prism {
                part: part.clone(),
                material: "cmu".into(),
                loops: model::one_loop(cell),
                kind: Kind::Ghost,
                pen: Some(Pen::Beyond),
                ..Prism::default()
            });
        }
        if k < n || top_joint {
            prisms.push(Prism {
                part: format!("joint_{k}"),
                material: "mortar".into(),
                loops: model::one_loop(model::rect_loop(0.0, y1, w, y1 + JOINT)),
                ..Prism::default()
            });
        }
        zones.push(zone_rect(&part, 0.0, y0, w, y1));
    }
    let mut anchors: Vec<NamedAnchor> = Vec::new();
    let bb_center = bond_beam_zones(wall, 0.0, w, &mut zones, &mut anchors);
    zones.extend(grout_zone(wall, fs, w - fs));
    let anchor = |name: &str, p: V2| NamedAnchor { name: name.into(), p };
    zones.push(zone_rect("cell", fs, 0.0, w - fs, total_h));
    anchors.splice(
        0..0,
        [
            anchor("bond_beam_center", bb_center),
            anchor("top_center", V2::init(w / 2.0, total_h)),
            anchor("cell_center_top", V2::init(w / 2.0, total_h)),
            anchor("cell_left", V2::init(fs, 0.0)),
            anchor("cell_center", V2::init(w / 2.0, 0.0)),
            anchor("cell_right", V2::init(w - fs, 0.0)),
            anchor("cell_left_top", V2::init(fs, total_h)),
            anchor("cell_right_top", V2::init(w - fs, total_h)),
        ],
    );
    Built {
        prisms,
        anchors,
        zones,
        box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: total_h },
        info: wall.info.clone(),
        ..Built::default()
    }
}

/// The wall seen face-on (run x): it runs along x for `length`, its thickness along z. Per panel (between control
/// joints) a front and a back face shell carrying the joint pattern (bed joints at every course, head joints in the
/// bond), and the grouted cells between them.
fn run_x(ctx: &mut Ctx, cp: &Params, wall: &Wall) -> Option<Built> {
    let (w, fs, n, total_h) = (wall.w, wall.fs, wall.n, wall.total_h);
    let Some(len) = cp.length else {
        ctx.p.missing("length", "run x draws the wall face-on along x, e.g. \"length\": 240");
        return None;
    };
    let mut cjs: Vec<f64> = Vec::new();
    if let Some(v) = &cp.control_joints {
        let Some(items) = v.arr() else {
            ctx.p.fail("control_joints", format!("param 'control_joints' must be a list of distances from the wall's left end, e.g. [96, 192] (got {})", model::kind_or_text(v)));
            return None;
        };
        for (i, e) in items.iter().enumerate() {
            let x = crate::units::parse_length(e);
            let prev = cjs.last().copied().unwrap_or(0.0);
            match x {
                Some(x) if x > prev + JOINT && x < len - JOINT => cjs.push(x),
                _ => {
                    ctx.p.fail(
                        &format!("control_joints/{i}"),
                        format!(
                            "control_joints[{i}] must be a distance along the wall, increasing, more than 3/8\" from the previous joint and inside the wall (0 to length {}) (got {})",
                            fmt_num(len),
                            model::kind_or_text(e)
                        ),
                    );
                    return None;
                }
            }
        }
    }
    let mut panels: Vec<(f64, f64)> = Vec::new();
    let mut x0 = 0.0;
    for &c in &cjs {
        panels.push((x0, c - JOINT / 2.0));
        x0 = c + JOINT / 2.0;
    }
    panels.push((x0, len));
    let units_h = n as f64 * COURSE - JOINT;
    let half = w / 2.0;
    let mut prisms: Vec<Prism> = Vec::new();
    for &(xa, xb) in &panels {
        let mut lines: Vec<[V2; 2]> = Vec::new();
        for k in 1..n {
            let y = k as f64 * COURSE - JOINT / 2.0;
            lines.push([V2::init(xa, y), V2::init(xb, y)]);
        }
        for k in 1..=n {
            let (y0, y1) = course_span(k, k);
            let y0 = if k == 1 { y0 } else { y0 - JOINT / 2.0 };
            let y1 = if k == n { y1 } else { y1 + JOINT / 2.0 };
            let off = if cp.bond == Bond::Running && k % 2 == 0 { COURSE } else { 0.0 };
            let mut x = xa + if off > 0.0 { off } else { 2.0 * COURSE } - JOINT / 2.0;
            while x < xb - 1e-6 {
                if x > xa + 1e-6 {
                    lines.push([V2::init(x, y0), V2::init(x, y1)]);
                }
                x += 2.0 * COURSE;
            }
        }
        for side in [1.0, -1.0] {
            prisms.push(Prism {
                material: "cmu".into(),
                loops: model::one_loop(model::rect_loop(xa, 0.0, xb, units_h)),
                zoff: side * (half - fs / 2.0),
                zhalf: fs / 2.0,
                face_lines: lines.clone(),
                face_on: true,
                ..Prism::default()
            });
        }
        for &(a, b) in &runs(&wall.grouted) {
            let (y0, y1) = course_span(a, b);
            prisms.push(Prism {
                part: "grout".into(),
                material: "grout".into(),
                loops: model::one_loop(model::rect_loop(xa, y0, xb, y1)),
                zhalf: half - fs,
                face_on: true,
                ..Prism::default()
            });
        }
    }
    if cp.top_joint {
        prisms.push(Prism {
            part: format!("joint_{n}"),
            material: "mortar".into(),
            loops: model::one_loop(model::rect_loop(0.0, units_h, len, total_h)),
            face_on: true,
            ..Prism::default()
        });
    }
    let mut zones = Vec::new();
    for k in 1..=n {
        let (y0, y1) = course_span(k, k);
        zones.push(zone_rect(&format!("course_{k}"), 0.0, y0, len, y1));
    }
    let mut anchors: Vec<NamedAnchor> = Vec::new();
    let bb_center = bond_beam_zones(wall, 0.0, len, &mut zones, &mut anchors);
    zones.extend(grout_zone(wall, 0.0, len));
    anchors.insert(0, NamedAnchor { name: "bond_beam_center".into(), p: bb_center });
    anchors.insert(1, NamedAnchor { name: "top_center".into(), p: V2::init(len / 2.0, total_h) });
    for (k, &c) in cjs.iter().enumerate() {
        anchors.push(NamedAnchor { name: format!("control_joint_{}", k + 1), p: V2::init(c, 0.0) });
        anchors.push(NamedAnchor { name: format!("control_joint_{}_top", k + 1), p: V2::init(c, total_h) });
    }
    let mut info = wall.info.clone();
    if !cjs.is_empty() {
        info.push_str(&format!(", {} control joint{}", cjs.len(), if cjs.len() == 1 { "" } else { "s" }));
    }
    Some(Built {
        prisms,
        anchors,
        zones,
        box_: Box { x0: 0.0, y0: 0.0, x1: len, y1: total_h },
        nat_z: Some(w),
        info,
        ..Built::default()
    })
}
