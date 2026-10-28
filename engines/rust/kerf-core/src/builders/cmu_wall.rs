//! cmu wall (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, fmt_num, parse_cover, zone_rect};
use crate::geom::{self, Box, V2};
use crate::model::{self, Built, HostInfo, Kind, NamedAnchor, Prism};
use crate::params::{param_enum, params};
use crate::pen::Pen;
use alloc::format;
use alloc::vec::Vec;

param_enum! { pub enum Grout { Solid = "solid", Reinforced = "reinforced", None = "none" } }

params! {
    pub struct Params {
        width: Num = 8.0 => { desc: "nominal 6, 8, 10, 12 => actual 5.625, 7.625, 9.625, 11.625" },
        courses: Int(u8, 1, 200) => { desc: "number of 8\" courses (7.625 unit + 0.375 mortar joint)" },
        bond_beam_courses: Int(u8, 0, 200) = 0 => { desc: "top N courses are bond-beam units (always grouted)" },
        grout: Enum(Grout) = Grout::Reinforced => { desc: "solid | reinforced (bond beams + the cut cell) | none" },
        face_shell: LenPos = 1.25 => { desc: "face shell thickness drawn in section" },
        top_joint: Bool = false => { desc: "mortar joint above the top course" },
        cover: OptRaw => { def: "{sides:1.5, top:1.5, bottom:0.5}", desc: "required clear cover for rebar (W_COVER); supports cover.parts.<part> overrides" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let cp = ctx.p.parse_all::<Params>();
    let cov = parse_cover(ctx, "cover", model::Cover { bottom: 0.5, sides: 1.5, top: 1.5 });
    if !ctx.p.ok {
        return None;
    }
    let cov = cov?;
    let x = cp.width;
    let w = if x == 6.0 {
        5.625
    } else if x == 8.0 {
        7.625
    } else if x == 10.0 {
        9.625
    } else if x == 12.0 {
        11.625
    } else if [5.625, 7.625, 9.625, 11.625].iter().any(|v| (x - v).abs() < 1e-3) {
        x
    } else {
        ctx.p.fail(
            "width",
            format!(
                "param 'width' must be a nominal 6, 8, 10 or 12 (actual 5.625, 7.625, 9.625, 11.625) (got {})",
                fmt_num(x)
            ),
        );
        return None;
    };
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
    let nf = n as f64;
    let total_h = nf * 8.0 - 0.375 + if cp.top_joint { 0.375 } else { 0.0 };
    let fs = cp.face_shell;
    let mut prisms: Vec<Prism> = Vec::new();
    let mut zones = Vec::new();
    for k in 1..=n {
        let y0 = (k - 1) as f64 * 8.0;
        let y1 = y0 + 7.625;
        let is_bb = k > n - nbb;
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
        let grouted = cp.grout != Grout::None && (is_bb || cp.grout == Grout::Solid || cp.grout == Grout::Reinforced);
        let cell = model::rect_loop(fs, y0, w - fs, y1);
        if grouted {
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
        if k < n || cp.top_joint {
            prisms.push(Prism {
                part: format!("joint_{k}"),
                material: "mortar".into(),
                loops: model::one_loop(model::rect_loop(0.0, y1, w, y1 + 0.375)),
                ..Prism::default()
            });
        }
        zones.push(zone_rect(&part, 0.0, y0, w, y1));
    }
    let mut bb_center = V2::init(w / 2.0, total_h);
    if nbb > 0 {
        let bb_y0 = (n - nbb) as f64 * 8.0;
        let y1 = nf * 8.0 - 0.375;
        zones.push(zone_rect("bond_beam", 0.0, bb_y0, w, y1));
        bb_center = V2::init(w / 2.0, (bb_y0 + y1) / 2.0);
    }
    {
        let mut gy0 = f64::INFINITY;
        let mut gy1 = f64::NEG_INFINITY;
        for pr in prisms.iter().filter(|pr| pr.part == "grout") {
            if let Some(l) = pr.loops.first() {
                let bx = geom::loop_box(l);
                gy0 = gy0.min(bx.y0);
                gy1 = gy1.max(bx.y1);
            }
        }
        if gy0 < gy1 {
            zones.push(zone_rect("grout", fs, gy0, w - fs, gy1));
        }
    }
    let anchor = |name: &str, p: V2| NamedAnchor { name: name.into(), p };
    let anchors = alloc::vec![
        anchor("bond_beam_center", bb_center),
        anchor("top_center", V2::init(w / 2.0, total_h)),
        anchor("cell_center_top", V2::init(w / 2.0, total_h)),
    ];
    let outline = model::rect_loop(0.0, 0.0, w, total_h);
    let nom = if (w - 5.625).abs() < 1e-3 {
        6.0
    } else if (w - 7.625).abs() < 1e-3 {
        8.0
    } else if (w - 9.625).abs() < 1e-3 {
        10.0
    } else {
        12.0
    };
    let bb_text = if nbb > 0 { format!(" ({nbb} bond beam)") } else { alloc::string::String::new() };
    Some(Built {
        prisms,
        anchors,
        zones,
        box_: Box { x0: 0.0, y0: 0.0, x1: w, y1: total_h },
        host: Some(HostInfo { outline, cover: cov.cover, part_cover: cov.parts }),
        info: format!("cmu_wall {}\" x {n} courses{bb_text}", fmt_num(nom)),
        ..Built::default()
    })
}
