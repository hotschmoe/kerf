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
param_enum! { pub enum Form { Wall = "wall", Pier = "pier" } }
param_enum! { pub enum Bond { Running = "running", Stack = "stack" } }

params! {
    pub struct Params {
        width: Num = 8.0 => { desc: "nominal 6, 8, 10, 12, 16 => actual 5.625, 7.625, 9.625, 11.625, 15.625" },
        form: Enum(Form) = Form::Wall => { desc: "wall: continuous wall; pier: square two-cell column unit, actual width by actual width (16 nominal = 15.625 square), run z, stack bond; z defaults to the actual unit depth" },
        units: OptRaw => { def: "null", desc: "special units by one-based course: {\"1\":\"lintel\",\"3\":\"inverted_bond_beam\"}. lintel: solid CMU bottom under the cell; inverted_bond_beam: solid top over the cell. Both always grouted. open_end, knockout: standard-unit fallback with W_PARAM and an exact removal op; describe the special unit in a construction note" },
        courses: Int(u8, 1, 200) => { desc: "number of 8\" courses (7.625 unit + 0.375 mortar joint)" },
        bond_beam_courses: Int(u8, 0, 200) = 0 => { desc: "top N courses are bond-beam units (always grouted)" },
        bond_beams: OptRaw => { def: "null", desc: "bond-beam courses anywhere in the wall (1 = bottom; always grouted): course numbers or [from, to] ranges, e.g. [4] (bond beam at a slab or ledger) or [[4, 5], 9]. Each run of adjacent bond-beam courses is a part bond_beam_<k> (k = 1 from the bottom) with anchor bond_beam_<k>_center; bond_beam is the highest. Run x also accepts {\"course\":3,\"x\":[0,72]} for a partial or stepped beam (local distances from wall left); adds to bond_beam_courses" },
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

fn course_span(a: usize, b: usize) -> (f64, f64) {
    ((a - 1) as f64 * COURSE, (b - 1) as f64 * COURSE + UNIT_H)
}

fn actual_width(ctx: &mut Ctx, x: f64) -> Option<f64> {
    for (nom, act) in [(6.0, 5.625), (8.0, 7.625), (10.0, 9.625), (12.0, 11.625), (16.0, 15.625)] {
        if x == nom || (x - act).abs() < 1e-3 {
            return Some(act);
        }
    }
    ctx.p.fail(
        "width",
        format!(
            "param 'width' must be a nominal 6, 8, 10, 12 or 16 (actual 5.625, 7.625, 9.625, 11.625, 15.625) (got {})",
            fmt_num(x)
        ),
    );
    None
}

#[derive(Clone)]
struct BeamSpan {
    course: usize,
    x: [f64; 2],
}

struct Wall {
    w: f64,
    fs: f64,
    n: usize,
    total_h: f64,
    bb: Vec<bool>,
    grouted: Vec<bool>,
    info: String,
    spans: Vec<BeamSpan>,
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
    if cp.form == Form::Pier && cp.face_shell * 3.0 >= w {
        ctx.p.fail(
            "face_shell",
            format!("a square two-cell pier needs face_shell less than one third of its {} actual width", fmt_num(w)),
        );
        return None;
    }
    if cp.face_shell * 2.0 >= w {
        ctx.p.fail(
            "face_shell",
            format!("face_shell {} leaves no cell in a {} wide wall", fmt_num(cp.face_shell), fmt_num(w)),
        );
        return None;
    }
    let mut spans = Vec::new();
    let mut full = Vec::new();
    if let Some(v) = cp.bond_beams.as_ref() {
        let Some(items) = v.arr() else {
            ctx.p.fail(
                "bond_beams",
                "bond_beams must be a list of courses, [from,to] ranges or {course,x:[start,end]} spans".into(),
            );
            return None;
        };
        for (i, v) in items.iter().enumerate() {
            if !matches!(v, Value::Object(_)) {
                full.push(v.clone());
                continue;
            }
            let course = v
                .get("course")
                .and_then(Value::num)
                .filter(|k| *k >= 1.0 && *k <= n as f64 && *k == crate::math::floor(*k));
            let x = v
                .get("x")
                .and_then(Value::arr)
                .filter(|x| x.len() == 2)
                .and_then(|x| Some([crate::units::parse_length(&x[0])?, crate::units::parse_length(&x[1])?]));
            match (course, x) {
                (Some(k), Some(x))
                    if cp.run == Run::X && x[0] >= 0.0 && x[1] > x[0] && x[1] <= cp.length.unwrap_or(0.0) =>
                {
                    spans.push(BeamSpan { course: k as usize, x })
                }
                _ => {
                    ctx.p.fail(
                        &format!("bond_beams/{i}"),
                        format!(
                            "partial beam needs run x, course 1..{n}, and x:[start,end] with 0 <= start < end <= length"
                        ),
                    );
                    return None;
                }
            }
        }
    }
    let mut bb = course_flags(ctx, "bond_beams", Some(&Value::Array(full)), n)?;
    for f in bb.iter_mut().skip(n - nbb) {
        *f = true;
    }
    if let Some(v) = cp.units.as_ref() {
        let Value::Object(entries) = v else {
            ctx.p.fail("units", "units must be an object keyed by one-based course, e.g. {\"1\":\"lintel\"}".into());
            return None;
        };
        for e in entries {
            let k = e.key.parse::<usize>().ok().filter(|&k| k >= 1 && k <= n);
            if let (Some(k), Some("lintel" | "inverted_bond_beam")) = (k, e.value.str()) {
                bb[k - 1] = true;
            } else if k.is_some() && matches!(e.value.str(), Some("open_end" | "knockout" | "knock_out")) {
                ctx.p.diags.add_fix(model::Level::Warning,"W_PARAM",Some(ctx.p.id),Some(&format!("{}/{}/units/{}",ctx.p.base,ctx.p.id,e.key)),
                    format!("course {} of '{}': {} units are not modelled; the drawing uses the standard two-cell profile",e.key,ctx.p.id,e.value.str().unwrap_or("special")),
                    format!("remove the unsupported selector and specify the special unit in a construction note: {{\"op\":\"update\",\"path\":\"components/{}\",\"value\":{{\"units\":{{\"{}\":null}}}}}}",ctx.p.id,e.key));
            } else {
                ctx.p.fail(&format!("units/{}",e.key), format!("units needs course 1..{n} and lintel or inverted_bond_beam; open-end and knock-out units are not modelled: remove this entry and describe the unit in a note"));
                return None;
            }
        }
    }
    if cp.form == Form::Pier && cp.run != Run::Z {
        ctx.p.fail("form", "form pier uses run z: remove run or set it to z".into());
        return None;
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
    let nom = [(5.625, 6.0), (7.625, 8.0), (9.625, 10.0), (15.625, 16.0)]
        .iter()
        .find(|(a, _)| (w - a).abs() < 1e-3)
        .map_or(12.0, |t| t.1);
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
    if cp.form == Form::Pier {
        info.push_str(&format!(", square two-cell pier {} x {}", fmt_num(w), fmt_num(w)));
    }
    if !spans.is_empty() {
        info.push_str(&format!(", {} partial bond beam spans", spans.len()));
    }
    let wall = Wall { w, fs: cp.face_shell, n, total_h, bb, grouted, info, spans };
    let mut built = match cp.run {
        Run::Z => run_z(&wall, cp.top_joint),
        Run::X => run_x(ctx, &cp, &wall)?,
    };
    if cp.form == Form::Pier {
        if let Some(z) = ctx.p.node.get("z").and_then(Value::arr).filter(|z| z.len() == 2)
            && let Some((a, b)) = crate::units::parse_length(&z[0]).zip(crate::units::parse_length(&z[1]))
            && ((b - a) - w).abs() > 1e-6
        {
            ctx.p.fail(
                "z",
                format!(
                    "a square pier is {} deep: use a z centre number, or a z range of that actual depth",
                    fmt_num(w)
                ),
            );
            return None;
        }
        built.nat_z = Some(w);
        for p in &mut built.prisms {
            p.cmu_unit = false;
        }
        let mut pier = Vec::new();
        for p in &built.prisms {
            if p.material != "grout" {
                pier.push(p.clone());
                continue;
            }
            let cell = (w - 3.0 * cp.face_shell) / 2.0;
            for z0 in [-w / 2.0 + cp.face_shell, cp.face_shell / 2.0] {
                let mut q = p.clone();
                q.zoff = z0 + cell / 2.0;
                q.zhalf = cell / 2.0;
                pier.push(q);
            }
        }
        for k in 1..=n {
            let (y0, y1) = course_span(k, k);
            for z0 in [-w / 2.0, -cp.face_shell / 2.0, w / 2.0 - cp.face_shell] {
                pier.push(Prism {
                    part: format!("course_{k}"),
                    material: "cmu".into(),
                    loops: model::one_loop(model::rect_loop(cp.face_shell, y0, w - cp.face_shell, y1)),
                    zoff: z0 + cp.face_shell / 2.0,
                    zhalf: cp.face_shell / 2.0,
                    ..Prism::default()
                });
            }
        }
        built.prisms = pier;
    }
    for k in 1..=n {
        let Some(unit) = cp.units.as_ref().and_then(|v| v.get(&format!("{k}"))).and_then(Value::str) else { continue };
        if !matches!(unit, "lintel" | "inverted_bond_beam") {
            continue;
        }
        let (y0, y1) = course_span(k, k);
        let (a, b) = if unit == "lintel" { (y0, y0 + cp.face_shell) } else { (y1 - cp.face_shell, y1) };
        let mut bottom = Prism {
            part: format!("course_{k}"),
            material: "cmu".into(),
            cmu_unit: cp.run == Run::Z && cp.form == Form::Wall,
            course: k as u32,
            ..Prism::default()
        };
        if cp.run == Run::X {
            bottom.loops = model::one_loop(model::rect_loop(0.0, a, built.box_.x1, b));
            bottom.zhalf = w / 2.0 - cp.face_shell;
        } else {
            bottom.loops = model::one_loop(model::rect_loop(cp.face_shell, a, w - cp.face_shell, b));
        }
        for p in built.prisms.iter_mut().filter(|p| p.material == "grout") {
            if let Some(l) = p.loops.first_mut() {
                let bx = geom::points_box(l);
                if (bx.y0 - y0).abs() < 1e-6 && (bx.y1 - y1).abs() < 1e-6 {
                    *l = model::rect_loop(
                        bx.x0,
                        if unit == "lintel" { b } else { y0 },
                        bx.x1,
                        if unit == "lintel" { y1 } else { a },
                    );
                }
            }
        }
        built.prisms.push(bottom);
    }
    let outline = model::rect_loop(0.0, 0.0, built.box_.x1, total_h);
    built.host = Some(HostInfo { outline, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set });
    Some(built)
}

/// Is course `k` (1 = bottom) a bond-beam course of this cmu_wall node? The plan view asks (SPEC 32.7).
pub fn is_bond_beam(node: &Value, k: usize) -> bool {
    let int = |v: &Value| v.num().map(|x| x as usize);
    let n = node.get("courses").and_then(int).unwrap_or(0);
    let top = node.get("bond_beam_courses").and_then(int).unwrap_or(0);
    if k + top > n && k <= n {
        return true;
    }
    if node
        .get("units")
        .and_then(|v| v.get(&format!("{k}")))
        .and_then(Value::str)
        .is_some_and(|v| matches!(v, "lintel" | "inverted_bond_beam"))
    {
        return true;
    }
    node.get("bond_beams").and_then(Value::arr).unwrap_or(&[]).iter().any(|e| match e.arr() {
        Some([a, b]) => int(a).zip(int(b)).is_some_and(|(a, b)| k >= a && k <= b),
        _ => int(e) == Some(k),
    })
}

pub fn solid_course(node: &Value, course: usize) -> bool {
    node.get("grout").and_then(Value::str) == Some("solid")
        || is_bond_beam(node, course)
        || node.get("grout_courses").and_then(Value::arr).unwrap_or(&[]).iter().any(|v| match v.arr() {
            Some([a, b]) => a.num().zip(b.num()).is_some_and(|(a, b)| course as f64 >= a && course as f64 <= b),
            _ => v.num() == Some(course as f64),
        })
}

pub fn beam_at(node: &Value, course: usize, x: f64) -> bool {
    is_bond_beam(node, course)
        || node.get("bond_beams").and_then(Value::arr).unwrap_or(&[]).iter().any(|e| {
            e.get("course").and_then(Value::num) == Some(course as f64)
                && e.get("x").and_then(Value::arr).is_some_and(|r| {
                    r.len() == 2
                        && crate::units::parse_length(&r[0])
                            .zip(crate::units::parse_length(&r[1]))
                            .is_some_and(|(a, b)| x >= a && x <= b)
                })
        })
}

fn bond_beam_zones(wall: &Wall, x0: f64, x1: f64, zones: &mut Vec<Zone>, anchors: &mut Vec<NamedAnchor>) -> V2 {
    let mut beams: Vec<(f64, f64, f64, f64)> = runs(&wall.bb)
        .into_iter()
        .map(|(a, b)| {
            let (y0, y1) = course_span(a, b);
            (y0, y1, x0, x1)
        })
        .collect();
    beams.extend(wall.spans.iter().map(|s| {
        let (y0, y1) = course_span(s.course, s.course);
        (y0, y1, s.x[0], s.x[1])
    }));
    crate::sort::stable(&mut beams, |a, b| a.0 < b.0 || (a.0 == b.0 && a.2 < b.2));
    let mut center = V2::init((x0 + x1) / 2.0, wall.total_h);
    for (k, &(y0, y1, a, b)) in beams.iter().enumerate() {
        zones.push(zone_rect(&format!("bond_beam_{}", k + 1), a, y0, b, y1));
        center = V2::init((a + b) / 2.0, (y0 + y1) / 2.0);
        anchors.push(NamedAnchor { name: format!("bond_beam_{}_center", k + 1), p: center });
    }
    if let Some(&(y0, y1, a, b)) = beams.last() {
        zones.push(zone_rect("bond_beam", a, y0, b, y1));
    }
    center
}

fn grout_zone(wall: &Wall, x0: f64, x1: f64) -> Option<Zone> {
    let rs = runs(&wall.grouted);
    if rs.is_empty() && wall.spans.is_empty() {
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
    for span in &wall.spans {
        let (y0, y1) = course_span(span.course, span.course);
        loops.push(model::rect_loop(span.x[0], y0, span.x[1], y1));
        bx.add_point(span.x[0], y0);
        bx.add_point(span.x[1], y1);
    }
    Some(Zone { name: "grout".into(), loops, box_: bx })
}

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
        let grouted_runs = if cp.units.is_some() {
            wall.grouted.iter().enumerate().filter(|(_, g)| **g).map(|(i, _)| (i + 1, i + 1)).collect()
        } else {
            runs(&wall.grouted)
        };
        for &(a, b) in &grouted_runs {
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
    let mut beams = wall.spans.clone();
    for (k, &bb) in wall.bb.iter().enumerate() {
        if bb {
            beams.push(BeamSpan { course: k + 1, x: [0.0, len] });
        }
    }
    let beams: Vec<BeamSpan> = beams
        .iter()
        .flat_map(|s| {
            panels.iter().filter_map(|&(a, b)| {
                let x = [s.x[0].max(a), s.x[1].min(b)];
                (x[1] > x[0]).then_some(BeamSpan { course: s.course, x })
            })
        })
        .collect();
    for span in &beams {
        let (y0, y1) = course_span(span.course, span.course);
        if !wall.grouted[span.course - 1] {
            prisms.push(Prism {
                part: "grout".into(),
                material: "grout".into(),
                loops: model::one_loop(model::rect_loop(span.x[0], y0, span.x[1], y1)),
                zhalf: half - fs,
                face_on: true,
                ..Prism::default()
            });
        }
        let inset = 1.0_f64.min((span.x[1] - span.x[0]) / 4.0);
        let line = model::rect_loop(span.x[0] + inset, y0 + 1.0, span.x[1] - inset, y1 - 1.0);
        let mut line = line;
        line.push(line[0]);
        prisms.push(Prism {
            part: "bond_beam".into(),
            material: "cmu".into(),
            kind: Kind::Ghost,
            pen: Some(Pen::Hidden),
            line_pts: line,
            zoff: half + 0.001,
            zhalf: 0.0001,
            ..Prism::default()
        });
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
