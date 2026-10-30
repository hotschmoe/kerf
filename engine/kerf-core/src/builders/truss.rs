//! truss (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{Ctx, fmt_ft_in, fmt_num, mirror_built, parse_sawn, zone_rect};
use crate::geom::{self, Box, Pt, V2};
use crate::json::Value;
use crate::math;
use crate::model::{self, Built, Kind, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use crate::pen::Pen;
use crate::units;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Exterior { Left = "left", Right = "right" } }
param_enum! { pub enum Heel { Standard = "standard", Raised = "raised" } }
param_enum! { pub enum Tail { Plumb = "plumb", Square = "square" } }

params! {
    pub struct Params {
        exterior: Enum(Exterior) = Exterior::Left => { desc: "side of the heel/overhang (right mirrors)" },
        pitch: Raw = Value::String("4:12".into()) => { def: "4:12", desc: "rise:run" },
        top_chord: Str = "2x4" => { desc: "sawn nominal size, depth in-plane" },
        bottom_chord: Str = "2x4" => { desc: "sawn nominal size, depth in-plane" },
        heel: Enum(Heel) = Heel::Standard => { desc: "standard | raised" },
        heel_height: OptLenPos => { desc: "raised heel: vertical height at the bearing outer edge from top of bottom chord to top of top chord" },
        bearing_width: LenPos = 3.5 => { desc: "width of the support under the heel" },
        overhang: Len = 12.0 => { desc: "horizontal distance from outer face of bearing to the tail end" },
        tail: Enum(Tail) = Tail::Plumb => { desc: "plumb | square cut" },
        span_shown: LenPos = 48.0 => { desc: "how far into the building to draw (crop/break at the end)" },
        plate: Bool = true => { desc: "draw the heel truss plate outline (dashed hidden pen)" },
    }
}

fn lower(x: f64, y0: f64, sl: f64) -> f64 {
    y0 + sl * x
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let tp = ctx.p.parse::<Params>()?;
    let top = tp.top_chord.as_str();
    let bot = tp.bottom_chord.as_str();
    let Some(theta) = units::parse_slope(&tp.pitch) else {
        ctx.p.fail(
            "pitch",
            format!("param 'pitch' must be rise:run like \"4:12\" or degrees (got {})", model::kind_or_text(&tp.pitch)),
        );
        return None;
    };
    if theta <= 0.0 || theta >= math::PI / 2.0 {
        ctx.p.fail("pitch", "pitch must slope up (between 0 and 90 degrees)".into());
        return None;
    }
    let Some(tsz) = parse_sawn(top) else {
        ctx.p.fail("top_chord", format!("top_chord \"{top}\" is not a sawn nominal size like \"2x4\", \"2x6\""));
        return None;
    };
    let Some(bsz) = parse_sawn(bot) else {
        ctx.p.fail("bottom_chord", format!("bottom_chord \"{bot}\" is not a sawn nominal size like \"2x4\", \"2x6\""));
        return None;
    };
    let dt = tsz.d;
    let db = bsz.d;
    let s = math::tan(theta);
    let c = math::cos(theta);
    let raised = tp.heel == Heel::Raised;
    let v_thick = dt / c;
    let mut y_low0 = db;
    if raised {
        let Some(hh) = tp.heel_height else {
            ctx.p.fail("heel_height", "heel 'raised' needs 'heel_height' (vertical height at the bearing outer edge from top of bottom chord to top of top chord)".into());
            return None;
        };
        if hh < v_thick {
            ctx.p.fail("heel_height", format!("heel_height {} is smaller than the top chord's vertical thickness {}; the raised heel must be at least that tall", fmt_num(hh), fmt_num(v_thick)));
            return None;
        }
        y_low0 = db + hh - v_thick;
    }
    let x_tail = -tp.overhang;
    let xe = tp.span_shown;
    let tail_bottom = V2::init(x_tail, lower(x_tail, y_low0, s));
    let mut tail_top = V2::init(x_tail, tail_bottom.y + v_thick);
    let tail_bottom_pt = tail_bottom;
    if tp.tail == Tail::Square {
        tail_top = tail_bottom.add(V2::init(-math::sin(theta), math::cos(theta)).scale(dt));
    }
    let top_loop = vec![
        Pt::at(tail_bottom_pt, 0.0),
        Pt::new(xe, lower(xe, y_low0, s)),
        Pt::new(xe, lower(xe, y_low0, s) + v_thick),
        Pt::at(tail_top, 0.0),
    ];
    let bot_loop = model::rect_loop(0.0, 0.0, xe, db);
    let mut prisms = vec![
        Prism {
            part: "bottom_chord".into(),
            material: "wood".into(),
            loops: model::one_loop(bot_loop),
            ..Prism::default()
        },
        Prism {
            part: "top_chord".into(),
            material: "wood".into(),
            loops: model::one_loop(top_loop.clone()),
            ..Prism::default()
        },
    ];
    let mut zones: Vec<Zone> = vec![
        zone_rect("bottom_chord", 0.0, 0.0, xe, db),
        Zone { name: "top_chord".into(), loops: model::one_loop(top_loop.clone()), box_: geom::loop_box(&top_loop) },
    ];
    if raised {
        let web_w = 1.5;
        let web =
            vec![Pt::new(0.0, db), Pt::new(web_w, db), Pt::new(web_w, lower(web_w, y_low0, s)), Pt::new(0.0, y_low0)];
        zones.push(Zone { name: "heel_web".into(), loops: model::one_loop(web.clone()), box_: geom::loop_box(&web) });
        prisms.push(Prism {
            part: "heel_web".into(),
            material: "wood".into(),
            loops: model::one_loop(web),
            ..Prism::default()
        });
    }
    if tp.plate {
        let py1 = (db + 0.5).max(y_low0 + 0.5);
        let pl = model::rect_loop(0.25, 0.25, 5.25, py1);
        prisms.push(Prism {
            part: "plate".into(),
            material: "steel".into(),
            loops: model::one_loop(pl),
            kind: Kind::Ghost,
            pen: Some(Pen::Hidden),
            embedded: true,
            ..Prism::default()
        });
        zones.push(zone_rect("plate", 0.25, 0.25, 5.25, py1));
    }
    {
        let mut tb = Box::default();
        tb.add_point(tail_bottom_pt.x, tail_bottom_pt.y);
        tb.add_point(tail_top.x, tail_top.y);
        tb.add_point(0.0, lower(0.0, y_low0, s));
        tb.add_point(0.0, lower(0.0, y_low0, s) + v_thick);
        zones.push(Zone {
            name: "tail".into(),
            loops: model::one_loop(model::rect_loop(tb.x0, tb.y0, tb.x1, tb.y1)),
            box_: tb,
        });
    }
    let anchor = |name: &str, p: V2| NamedAnchor { name: name.into(), p };
    let anchors = vec![
        anchor("bearing_outer", V2::init(0.0, 0.0)),
        anchor("bearing_inner", V2::init(tp.bearing_width, 0.0)),
        anchor("tail_bottom", tail_bottom_pt),
        anchor("tail_top", tail_top),
        anchor("top_chord_at_bearing", V2::init(0.0, lower(0.0, y_low0, s) + v_thick)),
        anchor("heel_outer", V2::init(0.0, (db + lower(0.0, y_low0, s) + v_thick) / 2.0)),
        anchor("top_chord_bottom_at_bearing", V2::init(0.0, lower(0.0, y_low0, s))),
        anchor("top_chord_end", V2::init(xe, lower(xe, y_low0, s) + v_thick)),
        anchor("bottom_chord_top_inner", V2::init(xe, db)),
    ];
    let mut box_ = Box::default();
    for pr in prisms.iter().filter(|pr| pr.kind != Kind::Ghost) {
        for l in &pr.loops {
            box_.add_box(geom::loop_box(l));
        }
    }
    let built = Built {
        prisms,
        anchors,
        zones,
        box_,
        nat_z: Some(tsz.t),
        info: format!(
            "truss {}:12 {} heel, {top}+{bot} chords, ovh {}",
            fmt_num(math::tan(theta) * 12.0),
            tp.heel.tag(),
            fmt_ft_in(tp.overhang)
        ),
        ..Built::default()
    };
    Some(if tp.exterior == Exterior::Right {
        mirror_built(&built, &geom::Xf::scaling(-1.0, 1.0), false)
    } else {
        built
    })
}
