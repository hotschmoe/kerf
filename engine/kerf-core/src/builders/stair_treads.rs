use super::common::{Ctx, fmt_ft_in, material_ok};
use super::steel_shapes::{self, Dims};
use crate::geom::{Box, V2};
use crate::model::{self, Built, NamedAnchor, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

param_enum! { pub enum Kind { Pan = "pan", Grating = "grating", Plate = "plate" } }

params! {
    pub struct Params {
        kind: Enum(Kind) = Kind::Pan => { desc: "pan: bent steel pan (riser and tread in one sheet) with a concrete fill; grating: bar grating treads with a nosing bar; plate: bent checkered plate treads with a turned-down nosing" },
        treads: Int(u8, 1, 60) = 3 => { desc: "number of treads drawn (a stub of the flight is usual: the crop cuts the stringers)" },
        rise: LenPos = 7.0 => { desc: "riser height (IBC 1011.5.2: 4 to 7; IRC R311.7.5.1: 7 3/4 max)" },
        tread: LenPos = 11.0 => { desc: "tread run, nosing to nosing (IBC 11 min; IRC 10 min)" },
        depth: OptLenPos => { def: "tread", desc: "front-to-back size of one tread when it differs from the run (grating and plate treads with open risers overlap or gap)" },
        thickness: OptLenPos => { def: "pan 0.1046 (12 ga.), grating 1.25, plate 0.25", desc: "sheet thickness (pan, plate) or bearing-bar depth (grating)" },
        fill: LenPos = 1.5 => { desc: "pan: depth of the concrete fill (its top is the walking surface)" },
        width: LenPos = 36.0 => { desc: "clear width between the stringers: the treads' extent along z (centered on z or the section's cut_z)" },
        carrier: Str = "L2x2x1/4" => { desc: "carrier angle under each tread on the stringers, an angle from the steel table, seen lengthwise against the far stringer; \"none\" omits them (treads welded directly to the stringers)" },
    }
}

fn steel(part: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> Prism {
    Prism {
        part: part.to_string(),
        material: "steel".to_string(),
        loops: model::one_loop(model::rect_loop(x0, y0, x1, y1)),
        ..Prism::default()
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let tp = ctx.p.parse::<Params>()?;
    let (rise, run) = (tp.rise, tp.tread);
    let depth = tp.depth.unwrap_or(run);
    let t = tp.thickness.unwrap_or(match tp.kind {
        Kind::Pan => 0.1046,
        Kind::Grating => 1.25,
        Kind::Plate => 0.25,
    });
    let angle = if tp.carrier.trim().eq_ignore_ascii_case("none") {
        None
    } else {
        match steel_shapes::lookup(&tp.carrier).map(|s| s.dims) {
            Ok(Dims::Angle { vert, t, .. }) => Some((vert, t)),
            Ok(_) => {
                ctx.p.fail(
                    "carrier",
                    format!(
                        "carrier \"{}\" is not an angle: give an angle such as \"L2x2x1/4\", or \"none\"",
                        tp.carrier
                    ),
                );
                return None;
            }
            Err(m) => {
                ctx.p.fail("carrier", format!("{m}; or \"none\" for no carrier angles"));
                return None;
            }
        }
    };
    let below = match tp.kind {
        Kind::Pan => tp.fill + t,
        _ => t,
    };
    if below >= rise || t >= depth / 2.0 || (tp.kind == Kind::Pan && depth != run) {
        let (k, m) = if tp.kind == Kind::Pan && depth != run {
            (
                "depth",
                "a pan stair's tread depth is its run (riser to riser): remove \"depth\" or use kind grating / plate"
                    .to_string(),
            )
        } else {
            (
                "thickness",
                format!(
                    "a {} tread {} deep (thickness{}) does not fit a {} rise and {} depth",
                    tp.kind.tag(),
                    fmt_ft_in(below),
                    if tp.kind == Kind::Pan { " + fill" } else { "" },
                    fmt_ft_in(rise),
                    fmt_ft_in(depth)
                ),
            )
        };
        ctx.p.fail(k, m);
        return None;
    }
    for m in if tp.kind == Kind::Pan { vec!["steel", "concrete"] } else { vec!["steel"] } {
        if !material_ok(ctx, "kind", m) {
            return None;
        }
    }
    let n = usize::from(tp.treads);
    let mut prisms: Vec<Prism> = Vec::new();
    let mut anchors = vec![NamedAnchor { name: "foot".to_string(), p: V2::init(0.0, 0.0) }];
    for k in 1..=n {
        let (x0, y) = ((k - 1) as f64 * run, k as f64 * rise);
        match tp.kind {
            Kind::Pan => {
                let yb = if k == 1 { 0.0 } else { y - rise - tp.fill - t };
                prisms.push(steel("riser", x0, yb, x0 + t, y));
                prisms.push(steel("tread", x0 + t, y - tp.fill - t, x0 + run, y - tp.fill));
                prisms.push(Prism {
                    part: "fill".to_string(),
                    material: "concrete".to_string(),
                    loops: model::one_loop(model::rect_loop(x0 + t, y - tp.fill, x0 + run, y)),
                    ..Prism::default()
                });
                if k == n {
                    prisms.push(steel("riser", x0 + run, y - tp.fill - t, x0 + run + t, y));
                }
            }
            Kind::Grating => {
                let nose = 1.25_f64.min(depth / 4.0);
                prisms.push(steel("nosing", x0, y - t, x0 + nose, y));
                let bars = crate::num::to_int_clamped::<usize>(crate::math::floor((depth - nose) / 1.1875), 0, 200);
                prisms.push(Prism {
                    lengthwise: true,
                    face_lines: (1..=bars)
                        .map(|i| x0 + nose + i as f64 * 1.1875)
                        .filter(|x| *x < x0 + depth - 0.2)
                        .map(|x| [V2::init(x, y - t), V2::init(x, y)])
                        .collect(),
                    ..steel("tread", x0 + nose, y - t, x0 + depth, y)
                });
            }
            Kind::Plate => {
                prisms.push(steel("tread", x0, y - t, x0 + depth, y));
                prisms.push(steel("nosing", x0, y - t - 1.25_f64.min(rise / 3.0), x0 + t, y - t));
            }
        }
        if let Some((leg, ta)) = angle {
            let (c0, c1) = (
                x0 + 1.0_f64.min(depth / 6.0) + if tp.kind == Kind::Pan { t } else { 0.0 },
                x0 + depth - 1.0_f64.min(depth / 6.0),
            );
            let top = y - below;
            for side in [-1.0, 1.0] {
                let zo = side * (tp.width / 2.0 - leg / 2.0);
                prisms.push(Prism {
                    zhalf: leg / 2.0,
                    zoff: zo,
                    lengthwise: true,
                    ..steel("carrier", c0, top - ta, c1, top)
                });
                prisms.push(Prism {
                    zhalf: ta / 2.0,
                    zoff: side * (tp.width / 2.0 - ta / 2.0),
                    lengthwise: true,
                    ..steel("carrier", c0, top - leg, c1, top - ta)
                });
            }
        }
        anchors.push(NamedAnchor { name: format!("nosing_{k}"), p: V2::init(x0, y) });
        anchors.push(NamedAnchor { name: format!("tread_{k}_back"), p: V2::init(x0 + depth, y) });
    }
    let (xt, yt) = ((n - 1) as f64 * run + depth, n as f64 * rise);
    anchors.push(NamedAnchor { name: "top".to_string(), p: V2::init(xt, yt) });
    let carriers = angle.map_or("no carrier angles".to_string(), |_| format!("carrier angles {}", tp.carrier.trim()));
    Some(Built {
        prisms,
        anchors,
        box_: Box { x0: 0.0, y0: 0.0, x1: xt, y1: yt },
        nat_z: Some(tp.width),
        info: format!(
            "stair treads ({}) {} {} @ {} rise x {} run, {} wide, {carriers}",
            tp.treads,
            tp.kind.tag(),
            fmt_ft_in(t),
            fmt_ft_in(rise),
            fmt_ft_in(run),
            fmt_ft_in(tp.width)
        ),
        ..Built::default()
    })
}
