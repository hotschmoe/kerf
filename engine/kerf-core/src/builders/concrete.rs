//! concrete (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{
    Ctx, drop_duplicate_points, fmt_num, fmt_ft_in, material_ok, mirror_built, one_prism, oriented_ccw, parse_cover,
    parse_point_list, zone_rect,
};
use crate::geom::{self, Pt, V2};
use crate::math;
use crate::model::{self, Built, HostInfo, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use crate::units;
use alloc::format;
use alloc::string::String;
use alloc::vec;

param_enum! { pub enum Shape { Rect = "rect", Footing = "footing", Polygon = "polygon", SlabEdge = "slab_edge" } }
param_enum! { pub enum Exterior { Left = "left", Right = "right" } }

params! {
    pub struct Params {
        shape: Enum(Shape) => { desc: "rect | footing | polygon | slab_edge" },
        material: Str = "concrete" => { desc: "any style material" },
        cover: OptRaw => { def: "{bottom:3, sides:3, top:1.5}", desc: "REQUIRED clear cover for bars in this host (W_COVER); cover.parts.<part> overrides per part zone, e.g. {\"parts\":{\"slab\":{\"bottom\":0.75}}}" },
        width: OptLenPos => { hint: "rect/footing need width and height", also: &["height"], def: "rect/footing: required", desc: "box size" },
        height: OptLenPos => { hint: "rect/footing need width and height", row: false, desc: "box size" },
        points: OptRaw => { def: "polygon: required", desc: "polyline [x,y(,bulge)] relative to the placement point, or absolute Refs" },
        exterior: Enum(Exterior) = Exterior::Left => { desc: "slab_edge: which side is the exterior edge (right mirrors)" },
        slab_thickness: LenPos = 4.0 => { desc: "slab_edge" },
        slab_length: LenPos = 48.0 => { desc: "slab_edge: slab drawn from exterior face inward" },
        footing_width: LenPos = 12.0 => { desc: "slab_edge: bottom width of the turndown" },
        footing_depth: LenPos = 18.0 => { desc: "slab_edge: top of slab to bottom of footing" },
        haunch: Num = 45.0 => { desc: "slab_edge: inner face slope from horizontal in degrees; 90 = vertical" },
        recess: OptRaw => { desc: "slab_edge: {width, depth, from_edge} depression at the top exterior edge (door sill); from_edge 0 = at the exterior face" },
        recess_slope: Len = 0.0 => { desc: "slab_edge: recess floor falls this many inches toward the exterior over its width" },
        base: OptRaw => { desc: "slab_edge: {material: gravel|sand|compacted_fill, thickness} uniform base course under the slab soffit and along the haunch (soil side), stopping at the footing bottom; part `base` with the fill hatch. Replaces hand-drawn fill polygons under the slab" },
    }
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let cp = ctx.p.parse_all::<Params>();
    let cov = parse_cover(ctx, "cover", model::Cover::default());
    if !ctx.p.ok {
        return None;
    }
    let cov = cov?;
    let material = cp.material.as_str();
    if !material_ok(ctx, "material", material) {
        return None;
    }
    let sh = cp.shape;
    if sh == Shape::Rect || sh == Shape::Footing {
        let hint = "rect/footing need width and height";
        let w = cp.width.unwrap_or_else(|| {
            ctx.p.missing("width", hint);
            0.0
        });
        let h = cp.height.unwrap_or_else(|| {
            ctx.p.missing("height", hint);
            0.0
        });
        if !ctx.p.ok {
            return None;
        }
        let lp = model::rect_loop(0.0, 0.0, w, h);
        let part = if sh == Shape::Footing { "footing" } else { "" };
        let prism = Prism {
            part: part.into(),
            material: material.into(),
            loops: model::one_loop(lp.clone()),
            ..Prism::default()
        };
        let zones = if part.is_empty() { vec![] } else { vec![zone_rect("footing", 0.0, 0.0, w, h)] };
        return Some(Built {
            prisms: one_prism(prism),
            zones,
            box_: geom::Box { x0: 0.0, y0: 0.0, x1: w, y1: h },
            host: Some(HostInfo { outline: lp, cover: cov.cover, part_cover: cov.parts }),
            info: format!("concrete {} {} x {}", sh.tag(), fmt_ft_in(w), fmt_ft_in(h)),
            ..Built::default()
        });
    }
    if sh == Shape::Polygon {
        let Some(pv) = &cp.points else {
            ctx.p.fail(
                "points",
                "shape polygon needs 'points': [[x, y], ...] (relative to the placement point) or Refs".into(),
            );
            return None;
        };
        let pts = parse_point_list(ctx, "points", pv, true)?;
        let clean = drop_duplicate_points(&pts);
        if clean.len() < 3 {
            ctx.p.fail("points", format!("polygon needs at least 3 distinct points (got {})", clean.len()));
            return None;
        }
        let lp = oriented_ccw(clean);
        let box_ = geom::loop_box(&lp);
        let prism = Prism { material: material.into(), loops: model::one_loop(lp.clone()), ..Prism::default() };
        return Some(Built {
            prisms: one_prism(prism),
            box_,
            points_mode: true,
            host: Some(HostInfo { outline: lp, cover: cov.cover, part_cover: cov.parts }),
            info: "concrete polygon".into(),
            ..Built::default()
        });
    }
    // slab_edge
    let st = cp.slab_thickness;
    let sl = cp.slab_length;
    let fw = cp.footing_width;
    let fd = cp.footing_depth;
    let haunch = cp.haunch;
    let (mut rw, mut rd, mut re) = (0.0, 0.0, 0.0);
    let mut has_recess = false;
    if let Some(rv) = &cp.recess {
        if !matches!(rv, crate::json::Value::Object(_)) {
            ctx.p
                .fail("recess", "param 'recess' must be {\"width\": w, \"depth\": d, \"from_edge\": e} or null".into());
        } else {
            has_recess = true;
            rw = rv.get("width").and_then(units::parse_length).unwrap_or_else(|| {
                ctx.p.fail("recess/width", "recess needs a numeric 'width'".into());
                0.0
            });
            rd = rv.get("depth").and_then(units::parse_length).unwrap_or_else(|| {
                ctx.p.fail("recess/depth", "recess needs a numeric 'depth'".into());
                0.0
            });
            re = ctx.p.field_len("recess", rv, "from_edge", 0.0).unwrap_or(0.0);
        }
    }
    let rslope = cp.recess_slope;
    let mut base_mat = String::new();
    let mut base_t = 0.0;
    if let Some(bv) = &cp.base {
        if !matches!(bv, crate::json::Value::Object(_)) {
            ctx.p.fail(
                "base",
                "param 'base' must be {\"material\": \"gravel\"|\"sand\"|\"compacted_fill\", \"thickness\": 4} or null"
                    .into(),
            );
        } else {
            base_mat = bv.get("material").and_then(|x| x.str()).unwrap_or("gravel").into();
            if !["gravel", "sand", "compacted_fill"].contains(&base_mat.as_str()) {
                ctx.p.fail(
                    "base/material",
                    format!("base.material must be \"gravel\", \"sand\" or \"compacted_fill\" (got \"{base_mat}\")"),
                );
            }
            base_t = bv.get("thickness").and_then(units::parse_length).unwrap_or_else(|| {
                ctx.p.fail(
                    "base/thickness",
                    "base needs a numeric 'thickness' (inches), e.g. {\"material\": \"gravel\", \"thickness\": 4}"
                        .into(),
                );
                0.0
            });
            if ctx.p.ok && base_t <= 0.0 {
                ctx.p.fail("base/thickness", "base.thickness must be greater than 0".into());
            }
        }
    }
    if !ctx.p.ok {
        return None;
    }
    if fd <= st {
        ctx.p.fail(
            "footing_depth",
            format!(
                "footing_depth ({}) must exceed slab_thickness ({}): it is measured from the top of the slab",
                fmt_num(fd),
                fmt_num(st)
            ),
        );
        return None;
    }
    if haunch <= 0.0 || haunch > 90.0 {
        ctx.p.fail(
            "haunch",
            format!("haunch must be an angle in degrees from horizontal, > 0 and <= 90 (got {})", fmt_num(haunch)),
        );
        return None;
    }
    if has_recess {
        if rw <= 0.0 || rd <= 0.0 || rd >= st {
            ctx.p.fail(
                "recess",
                format!(
                    "recess width must be > 0 and depth in (0, slab_thickness {}) (got width {}, depth {})",
                    fmt_num(st),
                    fmt_num(rw),
                    fmt_num(rd)
                ),
            );
            return None;
        }
        if re + rw > sl {
            ctx.p.fail(
                "recess",
                format!(
                    "recess (from_edge {} + width {}) extends beyond slab_length {}",
                    fmt_num(re),
                    fmt_num(rw),
                    fmt_num(sl)
                ),
            );
            return None;
        }
    }
    let hx = fw + (fd - st) / math::tan(haunch * geom::RAD_PER_DEG);
    if hx > sl {
        ctx.p.fail("haunch", format!("the haunch meets the slab underside at x={}, beyond slab_length {}; lengthen the slab or steepen haunch", fmt_num(hx), fmt_num(sl)));
        return None;
    }
    let mut pts: vec::Vec<Pt> = vec![Pt::new(0.0, -fd)];
    let rec_ext_y = -rd - rslope;
    if has_recess {
        if re > 0.0 {
            pts.push(Pt::new(0.0, 0.0));
            pts.push(Pt::new(re, 0.0));
        }
        pts.push(Pt::new(re, rec_ext_y));
        pts.push(Pt::new(re + rw, -rd));
        pts.push(Pt::new(re + rw, 0.0));
    } else {
        pts.push(Pt::new(0.0, 0.0));
    }
    pts.push(Pt::new(sl, 0.0));
    pts.push(Pt::new(sl, -st));
    pts.push(Pt::new(hx, -st));
    pts.push(Pt::new(fw, -fd));
    let lp = oriented_ccw(pts);
    let prism = Prism { material: material.into(), loops: model::one_loop(lp.clone()), ..Prism::default() };
    let anchor = |name: &str, p: V2| NamedAnchor { name: name.into(), p };
    let mut anchors = vec![
        anchor("top_exterior", V2::init(0.0, 0.0)),
        anchor("slab_top", V2::init(sl, 0.0)),
        anchor("footing_bottom_exterior", V2::init(0.0, -fd)),
        anchor("footing_bottom_interior", V2::init(fw, -fd)),
        anchor("slab_bottom_interior", V2::init(sl, -st)),
        anchor("haunch_top", V2::init(hx, -st)),
    ];
    if has_recess {
        anchors.push(anchor("recess_bottom_exterior", V2::init(re, rec_ext_y)));
        anchors.push(anchor("recess_bottom_interior", V2::init(re + rw, -rd)));
        anchors.push(anchor("recess_top_interior", V2::init(re + rw, 0.0)));
    }
    let mut prisms_out = vec![prism];
    let mut zones_out: vec::Vec<Zone> =
        vec![zone_rect("footing", 0.0, -fd, fw, -st), zone_rect("slab", 0.0, -st, sl, 0.0)];
    if base_t > 0.0 {
        let dx = hx - fw;
        let dy = fd - st;
        let dl = math::sqrt(dx * dx + dy * dy);
        let ux = dx / dl;
        let uy = dy / dl;
        let nx = uy;
        let ny = -ux;
        let o = V2::init(fw + nx * base_t, -fd + ny * base_t);
        if st + base_t >= fd - 1e-9 {
            ctx.p.fail("base/thickness", format!("base.thickness {} must be less than footing_depth - slab_thickness ({}) so the base stops above the footing bottom", fmt_num(base_t), fmt_num(fd - st)));
            return None;
        }
        let s1 = (-st - base_t - o.y) / uy;
        let s2 = (-fd - o.y) / uy;
        let q1 = V2::init(o.x + ux * s1, -st - base_t);
        let q2 = V2::init(o.x + ux * s2, -fd);
        if q1.x >= sl - 1e-9 {
            ctx.p.fail("base/thickness", "the base course meets the slab end before the haunch offset does; lengthen slab_length or reduce base.thickness".into());
            return None;
        }
        let bpts = [
            Pt::new(fw, -fd),
            Pt::new(hx, -st),
            Pt::new(sl, -st),
            Pt::new(sl, -st - base_t),
            Pt::new(q1.x, q1.y),
            Pt::new(q2.x, q2.y),
        ];
        let bloop = oriented_ccw(drop_duplicate_points(&bpts));
        let bb = geom::loop_box(&bloop);
        zones_out.push(Zone { name: "base".into(), loops: model::one_loop(bloop.clone()), box_: bb });
        prisms_out.push(Prism {
            part: "base".into(),
            material: base_mat.clone(),
            loops: model::one_loop(bloop),
            ..Prism::default()
        });
        anchors.push(anchor("base_bottom_interior", V2::init(sl, -st - base_t)));
        anchors.push(anchor("base_bottom_footing", V2::init(q2.x, -fd)));
    }
    let recess_text = if has_recess { format!(", recess {} x {}", fmt_ft_in(rw), fmt_ft_in(rd)) } else { String::new() };
    let base_text = if base_t > 0.0 { format!(", {} {base_mat} base", fmt_ft_in(base_t)) } else { String::new() };
    let mut built = Built {
        prisms: prisms_out,
        anchors,
        zones: zones_out,
        box_: geom::loop_box(&lp),
        host: Some(HostInfo { outline: lp, cover: cov.cover, part_cover: cov.parts }),
        info: format!("concrete slab_edge {} slab, ftg {} x {}{recess_text}{base_text}", fmt_ft_in(st), fmt_ft_in(fw), fmt_ft_in(fd)),
        ..Built::default()
    };
    if cp.exterior == Exterior::Right {
        built = mirror_built(&built, &geom::Xf::scaling(-1.0, 1.0), false);
        if let Some(h) = &built.host {
            built.box_ = geom::loop_box(&h.outline);
        }
    }
    Some(built)
}
