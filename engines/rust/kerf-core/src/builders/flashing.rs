//! flashing (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, drop_duplicate_points, gauge_thickness, material_or, mirror_built, parse_point_list};
use crate::model::fmt_num;
use crate::geom::{self, Pt, V2, Xf};
use crate::model::{self, Built, NamedAnchor, Prism};
use crate::num;
use crate::params::{ParamEnum, param_enum, params};
use crate::pathgeom;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Profile { Z = "z", L = "l", Drip = "drip", WeepScreed = "weep_screed", Points = "points" } }
param_enum! { pub enum Exterior { Left = "left", Right = "right" } }

params! {
    pub struct Params {
        profile: Enum(Profile) = Profile::Z => { desc: "z: back flange up the wall, horizontal leg out, drop at the nose; l: flange + horizontal leg; drip: flange on the deck, drop, outward kick; weep_screed: nailing flange up the wall, ledge, small drip drop; points: free centerline polyline" },
        flange: OptLenPos => { def: "2 (weep_screed 3.5)", desc: "vertical back/nailing flange length (drip: horizontal flange on the deck)" },
        leg: OptLenPos => { def: "1 (l 2)", desc: "horizontal leg length toward the exterior" },
        drop: OptLenPos => { def: "2 (drip 1.5, weep_screed 0.5)", desc: "downturned leg at the nose" },
        kick: LenPos = 0.5 => { desc: "drip only: outward kick at the bottom of the drop" },
        gauge: Num = 26.0 => { desc: "20 .0359, 22 .0299, 24 .0239, 26 .0179, 28 .0149" },
        exterior: Enum(Exterior) = Exterior::Left => { desc: "side the nose faces (right mirrors); presets only" },
        points: OptRaw => { def: "profile points: required", desc: "centerline polyline [x,y] relative to the placement point, or Refs" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let fp = ctx.p.parse::<Params>()?;
    let pr = fp.profile;
    let flange = fp.flange.unwrap_or(if pr == Profile::WeepScreed { 3.5 } else { 2.0 });
    let leg = fp.leg.unwrap_or(if pr == Profile::L { 2.0 } else { 1.0 });
    let drop = fp.drop.unwrap_or(match pr {
        Profile::Drip => 1.5,
        Profile::WeepScreed => 0.5,
        _ => 2.0,
    });
    let kick = fp.kick;
    let gauge_i = num::to_int::<u32>(crate::math::round(fp.gauge)).unwrap_or(0);
    let Some(thickness) = gauge_thickness(gauge_i) else {
        ctx.p.fail("gauge", format!("gauge {} is not in the table; use 20, 22, 24, 26 (default, 0.0179\") or 28", fmt_num(fp.gauge)));
        return None;
    };
    let mut pts: Vec<Pt> = Vec::new();
    let mut points_mode = false;
    match pr {
        Profile::Z | Profile::WeepScreed => {
            pts.extend([Pt::new(0.0, flange), Pt::new(0.0, 0.0), Pt::new(-leg, 0.0), Pt::new(-leg, -drop)]);
        }
        Profile::L => pts.extend([Pt::new(0.0, flange), Pt::new(0.0, 0.0), Pt::new(-leg, 0.0)]),
        Profile::Drip => pts.extend([Pt::new(flange, 0.0), Pt::new(0.0, 0.0), Pt::new(0.0, -drop), Pt::new(-kick, -drop - 0.5 * kick)]),
        Profile::Points => {
            let Some(pv) = fp.points.as_ref() else {
                ctx.p.fail("points", "profile \"points\" needs 'points': the sheet-metal centerline polyline, e.g. [[0,2],[0,0],[-1,0],[-1,-2]] (or Refs)".to_string());
                return None;
            };
            let pl = parse_point_list(ctx, "points", pv, true)?;
            let clean = drop_duplicate_points(&pl);
            if clean.len() < 2 {
                ctx.p.fail("points", format!("flashing points need at least 2 distinct points (got {})", clean.len()));
                return None;
            }
            pts.extend(clean);
            points_mode = true;
        }
    }
    let rib = pathgeom::ribbon(&pts, thickness / 2.0, thickness / 2.0);
    let mat = material_or(ctx, "steel", "generic");
    let first = pts.first()?.v();
    let last = pts.last()?.v();
    let anchors = vec![
        NamedAnchor { name: "corner".to_string(), p: if points_mode { first } else { V2::init(0.0, 0.0) } },
        NamedAnchor { name: "start".to_string(), p: first },
        NamedAnchor { name: "end".to_string(), p: last },
    ];
    let box_ = geom::loop_box(&rib);
    let prism = Prism { material: mat.to_string(), loops: model::one_loop(rib), embedded: true, centerline: pts, ..Prism::default() };
    let mut built = Built {
        prisms: vec![prism],
        anchors,
        box_,
        points_mode,
        info: format!("flashing {} {} ga ({}\" thick)", pr.tag(), gauge_i, fmt_num(thickness)),
        ..Built::default()
    };
    if !points_mode && fp.exterior == Exterior::Right {
        built = mirror_built(&built, &Xf::scaling(-1.0, 1.0), false);
    }
    Some(built)
}
