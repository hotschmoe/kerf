//! concrete (SPEC 5): its parameters (`Params`, the single source for the parser and the catalog) and its builder.

use super::common::{
    Ctx, ParsedCover, drop_duplicate_points, fmt_ft_in, fmt_num, material_ok, mirror_built, one_prism, oriented_ccw,
    parse_cover, parse_point_list, vertex_anchors, zone_rect,
};
use crate::geom::{self, Pt, V2};
use crate::math;
use crate::model::{self, Built, HostInfo, NamedAnchor, Prism, Zone};
use crate::params::{ParamEnum, param_enum, params};
use crate::units;
use alloc::format;
use alloc::string::String;
use alloc::vec;

param_enum! { pub enum Shape { Rect = "rect", Footing = "footing", Polygon = "polygon", SlabEdge = "slab_edge", Footprint = "footprint" } }
param_enum! { pub enum Exterior { Left = "left", Right = "right" } }

params! {
    pub struct Params {
        shape: Enum(Shape) => { desc: "rect | footing | polygon | slab_edge | footprint (a plan outline [x, z] with a thickness in y: L and T footings, notched slabs, block-outs with 45-degree ends, angled walls)" },
        material: Str = "concrete" => { desc: "any style material" },
        cover: OptRaw => { def: "{bottom:3, sides:3, top:1.5}", desc: "REQUIRED clear cover for bars in this host (W_COVER); cover.parts.<part> overrides per part zone, e.g. {\"parts\":{\"slab\":{\"bottom\":0.75}}}. The default suits footings and walls cast against earth (every shape, polygons included); give a slab its own, e.g. a 4\" slab on grade with a mid-depth bar: {\"bottom\":1.5,\"top\":1.5}" },
        width: OptLenPos => { hint: "rect/footing need width and height", also: &["height"], def: "rect/footing: required", desc: "box size; footprint: height is the thickness in y" },
        height: OptLenPos => { hint: "rect/footing need width and height", row: false, desc: "box size" },
        points: OptRaw => { def: "polygon, footprint: required", desc: "polygon: polyline [x,y(,bulge)] relative to the placement point, or absolute Refs. footprint: the plan outline as [x, z(, bulge)] pairs (model x and z; x relative to the placement point)" },
        top: OptRaw => { def: "footprint: 0", desc: "footprint: y of the top surface, a length (relative to the placement point) or a Ref (its y); the footprint spans top - height .. top" },
        exterior: Enum(Exterior) = Exterior::Left => { desc: "slab_edge: which side is the exterior edge (right mirrors)" },
        slab_thickness: LenPos = 4.0 => { desc: "slab_edge" },
        slab_length: LenPos = 48.0 => { desc: "slab_edge: slab drawn from exterior face inward" },
        footing_width: LenPos = 12.0 => { desc: "slab_edge: bottom width of the turndown" },
        footing_depth: LenPos = 18.0 => { desc: "slab_edge: top of slab to bottom of footing" },
        haunch: Num = 45.0 => { desc: "slab_edge: inner face slope from horizontal in degrees; 90 = vertical" },
        recess: OptRaw => { desc: "slab_edge: {width, depth, from_edge} depression at the top exterior edge (door sill); from_edge 0 = at the exterior face" },
        recess_slope: Len = 0.0 => { desc: "slab_edge: recess floor falls this many inches toward the exterior over its width" },
        base: OptRaw => { desc: "slab_edge: {material: gravel|sand|compacted_fill, thickness} uniform base course under the slab soffit and along the haunch (soil side), stopping at the footing bottom; part `base` with the fill hatch. Replaces hand-drawn fill polygons under the slab" },
        pour: OptStr => { def: "null", desc: "monolithic pour: concrete components with the same pour name are one casting, so where they touch no joint line is drawn (section, plan, elevation), e.g. a slab and its thickened edge, or the legs of an L footing" },
        voids: OptRaw => { def: "null", desc: "what the host loses (SPEC 26.4): component ids (\"column\", \"plate.part\": its outline over its z extent, so the hole follows the member) and shapes {\"rect\": [w, h], \"at\": Ref, \"anchor\": \"center\"} | {\"circle\": d, \"at\": Ref} | {\"points\": [...]}, each with optional \"z\": [z0, z1] (default: through). Cut in section, iso and 3D; embedded steel then shows with its own fill. W_COVER measures to the void edge" },
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
    match cp.shape {
        sh @ (Shape::Rect | Shape::Footing) => rect_or_footing(ctx, &cp, sh, material, cov),
        Shape::Polygon => polygon(ctx, &cp, material, cov),
        Shape::Footprint => footprint(ctx, &cp, material, cov),
        _ => slab_edge(ctx, &cp, material, cov),
    }
}

/// The two plain blocks: a rectangle, or the same with the `footing` part (and its zone).
fn rect_or_footing(ctx: &mut Ctx, cp: &Params, sh: Shape, material: &str, cov: ParsedCover) -> Option<Built> {
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
    let prism =
        Prism { part: part.into(), material: material.into(), loops: model::one_loop(lp.clone()), ..Prism::default() };
    let zones = if part.is_empty() { vec![] } else { vec![zone_rect("footing", 0.0, 0.0, w, h)] };
    Some(Built {
        prisms: one_prism(prism),
        zones,
        box_: geom::Box { x0: 0.0, y0: 0.0, x1: w, y1: h },
        host: Some(HostInfo { outline: lp, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set }),
        info: format!("concrete {} {} x {}", sh.tag(), fmt_ft_in(w), fmt_ft_in(h)),
        ..Built::default()
    })
}

fn polygon(ctx: &mut Ctx, cp: &Params, material: &str, cov: ParsedCover) -> Option<Built> {
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
    let anchors = vertex_anchors(&clean, true);
    let lp = oriented_ccw(clean);
    let box_ = geom::loop_box(&lp);
    let prism = Prism { material: material.into(), loops: model::one_loop(lp.clone()), ..Prism::default() };
    Some(Built {
        prisms: one_prism(prism),
        anchors,
        box_,
        points_mode: true,
        host: Some(HostInfo { outline: lp, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set }),
        info: "concrete polygon".into(),
        ..Built::default()
    })
}

/// Most slabs one z interval of a footprint with an angled edge is cut into for the checks; all intervals together.
const FP_STEPS_MAX: usize = 48;
const FP_SLABS_MAX: usize = 400;

/// Whether two segments cross or touch away from a shared end (a footprint must be a simple polygon).
fn segments_meet(a0: V2, a1: V2, b0: V2, b1: V2) -> bool {
    let mut ta = [0.0; 2];
    let mut tb = [0.0; 2];
    let n = geom::seg_seg(a0, a1, b0, b1, &mut ta, &mut tb);
    (0..n).any(|k| {
        let end_a = ta[k] < 1e-9 || ta[k] > 1.0 - 1e-9;
        let end_b = tb[k] < 1e-9 || tb[k] > 1.0 - 1e-9;
        !(end_a && end_b)
    })
}

/// A plan footprint (SPEC 28.7): an outline in [x, z] with a thickness in y. Its prisms are z slabs, one per run of
/// z between the outline's vertices, with the outline's x intervals; an angled edge makes a staircase of thin slabs
/// for the checks, and section, plan, elevation, iso and 3D draw the exact outline carried on the first slab.
fn footprint(ctx: &mut Ctx, cp: &Params, material: &str, cov: ParsedCover) -> Option<Built> {
    if ctx.angle != 0.0 {
        ctx.p.fail("rotate", "a footprint is drawn in plan ([x, z] points) and rotate/slope tilt the section plane: give the angled outline in 'points' instead".into());
        return None;
    }
    let Some(h) = cp.height else {
        ctx.p.missing("height", "footprint needs 'height', its thickness in y below 'top'");
        return None;
    };
    let top = match &cp.top {
        None => 0.0,
        Some(v) => match units::parse_length(v) {
            Some(y) => y,
            None => {
                let path = format!("{}/{}/top", ctx.p.base, ctx.p.id);
                let id = String::from(ctx.p.id);
                ctx.scene.point(v, &id, &path)?.y - ctx.origin.y
            }
        },
    };
    let example = "e.g. \"points\": [[0, 0], [48, 0], [48, -12], [12, -12], [12, -48], [0, -48]] (an L)";
    let Some(arr) = cp.points.as_ref().and_then(|v| v.arr()) else {
        ctx.p.fail(
            "points",
            format!("shape footprint needs 'points': the plan outline as [x, z] pairs (model x and z), {example}"),
        );
        return None;
    };
    let mut lp: vec::Vec<Pt> = vec::Vec::new();
    for (i, e) in arr.iter().enumerate() {
        let xz = e.arr().filter(|a| a.len() == 2 || a.len() == 3).and_then(|a| {
            let b = match a.get(2) {
                None => 0.0,
                Some(bv) => bv.num().filter(|b| {
                    *b == 0.0 || (b.abs() >= super::common::MIN_BULGE && b.abs() <= super::common::MAX_BULGE)
                })?,
            };
            Some(Pt { x: units::parse_length(&a[0])?, y: units::parse_length(&a[1])?, b })
        });
        let Some(q) = xz else {
            ctx.p.fail(&format!("points/{i}"), format!("point {i} of a footprint must be [x, z] or [x, z, bulge]: lengths (model x and z, seen from above), a bulge 0 or 0.000001..1000 (got {}); Refs are not taken", model::kind_or_text(e)));
            return None;
        };
        lp.push(q);
    }
    let clean = drop_duplicate_points(&lp);
    if clean.len() < 3 {
        ctx.p.fail("points", format!("a footprint needs at least 3 distinct points (got {}), {example}", clean.len()));
        return None;
    }
    let mut poly = geom::flatten_loop(&clean, geom::FLAT_TOL);
    if geom::signed_area_v(&poly) < 0.0 {
        poly.reverse();
    }
    let n = poly.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if segments_meet(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                ctx.p.fail(
                    "points",
                    "the footprint outline crosses itself: list the points once around the outline, in order".into(),
                );
                return None;
            }
        }
    }
    let area = geom::signed_area_v(&poly);
    if area < 1e-6 {
        ctx.p.fail("points", "the footprint outline has no area".into());
        return None;
    }
    let angled = (0..n).any(|i| {
        let d = poly[(i + 1) % n].sub(poly[i]);
        d.x.abs() > 1e-9 && d.y.abs() > 1e-9
    });
    let mut zs: vec::Vec<f64> = poly.iter().map(|v| v.y).collect();
    crate::sort::stable(&mut zs, |a, b| a < b);
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let (zmin, zmax) = (zs[0], zs[zs.len() - 1]);
    let zc = (zmin + zmax) / 2.0;
    let total_dz: f64 = zmax - zmin;
    let step = 1.0f64.max(total_dz / FP_SLABS_MAX as f64);
    let (y0, y1) = (top - h, top);
    let mut prisms: vec::Vec<Prism> = vec::Vec::new();
    for w in zs.windows(2) {
        let (za, zb) = (w[0], w[1]);
        let zm = (za + zb) / 2.0;
        let slanted = (0..n).any(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            (p.y.min(q.y) < zm && p.y.max(q.y) > zm) && (q.x - p.x).abs() > 1e-9
        });
        let k_n = if slanted {
            crate::num::to_int_clamped::<usize>(math::ceil((zb - za) / step), 1, FP_STEPS_MAX)
        } else {
            1
        };
        for k in 0..k_n {
            let a = za + (zb - za) * k as f64 / k_n as f64;
            let b = za + (zb - za) * (k + 1) as f64 / k_n as f64;
            let xs = crate::pathclip::scan(core::slice::from_ref(&poly), (a + b) / 2.0, false);
            for iv in xs.as_chunks::<2>().0 {
                if iv[1] - iv[0] < 1e-9 {
                    continue;
                }
                prisms.push(Prism {
                    material: material.into(),
                    loops: model::one_loop(model::rect_loop(iv[0], y0, iv[1], y1)),
                    zoff: (a + b) / 2.0 - zc,
                    zhalf: (b - a) / 2.0,
                    seam: [a > zmin + 1e-9, b < zmax - 1e-9],
                    fp_proxy: angled,
                    ..Prism::default()
                });
            }
        }
    }
    if let Some(first) = prisms.first_mut().filter(|_| angled) {
        first.footprint = poly.iter().map(|v| V2::init(v.x, v.y - zc)).collect();
        first.fp_y = [y0, y1];
    }
    let bx = geom::points_box(&poly.iter().map(|v| Pt::at(*v, 0.0)).collect::<vec::Vec<_>>());
    let box_ = geom::Box { x0: bx.x0, y0, x1: bx.x1, y1 };
    let mut anchors = vec::Vec::new();
    let mut plan_z = vec::Vec::new();
    for (k, v) in clean.iter().enumerate() {
        let w = clean[(k + 1) % clean.len()];
        let mid = geom::seg_point(v.v(), w.v(), v.b, 0.5);
        for (name, p) in [(format!("vertex_{}", k + 1), v.v()), (format!("edge_{}_mid", k + 1), mid)] {
            anchors.push(NamedAnchor { name: name.clone(), p: V2::init(p.x, top) });
            plan_z.push(NamedAnchor { name, p: V2::init(0.0, p.y - zc) });
        }
    }
    let outline = model::rect_loop(box_.x0, y0, box_.x1, y1);
    Some(Built {
        prisms,
        anchors,
        plan_z,
        box_,
        nat_z: Some(total_dz),
        nat_zc: Some(zc),
        points_mode: true,
        host: Some(HostInfo { outline, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set }),
        info: format!(
            "concrete footprint {} points, {} thick, z {}..{}",
            clean.len(),
            fmt_ft_in(h),
            fmt_ft_in(zmin),
            fmt_ft_in(zmax)
        ),
        ..Built::default()
    })
}

/// A slab with an optional footing, haunch and recess along its edge.
fn slab_edge(ctx: &mut Ctx, cp: &Params, material: &str, cov: ParsedCover) -> Option<Built> {
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
    let recess_text =
        if has_recess { format!(", recess {} x {}", fmt_ft_in(rw), fmt_ft_in(rd)) } else { String::new() };
    let base_text = if base_t > 0.0 { format!(", {} {base_mat} base", fmt_ft_in(base_t)) } else { String::new() };
    let mut built = Built {
        prisms: prisms_out,
        anchors,
        zones: zones_out,
        box_: geom::loop_box(&lp),
        host: Some(HostInfo { outline: lp, cover: cov.cover, part_cover: cov.parts, cover_set: cov.set }),
        info: format!(
            "concrete slab_edge {} slab, ftg {} x {}{recess_text}{base_text}",
            fmt_ft_in(st),
            fmt_ft_in(fw),
            fmt_ft_in(fd)
        ),
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
