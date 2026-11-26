use super::common::{Ctx, fmt_ft_in, gauge_thickness, length_or_until, material_ok, oriented_ccw, parse_cover};
use crate::geom::{Box, Pt, V2};
use crate::model::{self, Built, Cover, HostInfo, NamedAnchor, Prism};
use crate::params::{ParamEnum, param_enum, params};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

param_enum! { pub enum Run { Z = "z", X = "x" } }

params! {
    pub struct Params {
        profile: Str = "1.5B" => { desc: "deck type: 1.5B (wide rib roof deck, 6 pitch), 1.5F (intermediate rib), 1.5A (narrow rib), 3N (deep rib, 8 pitch), 2C / 3C (2 and 3 composite floor deck, 12 pitch; also 2VLI, 3VLI, 2W, 3W), 1.5VLI (1 1/2 composite, 6 pitch), 9/16 (form deck, 2 1/2 pitch; also 0.6C). Another profile: any name here for the notes plus depth, pitch, rib_bottom and rib_top" },
        gauge: Int(u8, 10, 28) = 22 => { desc: "sheet gauge: 22, 20, 18, 16 (roof and composite deck), 26, 24, 28 (form deck)" },
        run: Enum(Run) = Run::Z => { desc: "direction of the flutes. z: into the drawing, the deck is cut across its flutes and the fluted profile shows (deck on joists or beams seen in elevation: the deck spans between them); x: along the drawing, the deck is seen along a flute as its top and bottom sheet lines (deck on joists or beams cut end-on)" },
        length: OptRaw => { def: "required", desc: "extent along x (or until): the width of deck drawn for run z, the length along the flutes for run x" },
        until: OptRaw => { desc: "alternative to length: a Ref; the deck grows from its placement anchor until its far end reaches the Ref's x" },
        offset: Len = 0.0 => { desc: "run z: shifts the flute pattern along x. At 0 the left edge is the middle of a top flat and the first rib (bottom flat) is centered at pitch / 2" },
        topping: OptLenPos => { desc: "concrete over the top of the flutes (total slab depth = deck depth + topping), cast into the flutes: part topping, drawn with the concrete hatch and a host for rebar or mesh (cover)" },
        topping_material: Str = "concrete" => { desc: "material of the topping (a style material: concrete, insulation_rigid ...)" },
        cover: OptRaw => { def: "{bottom:0.75, sides:0, top:0.75}", desc: "with topping: clear cover for bars or mesh hosted in it (W_COVER), as concrete cover. Sides default 0 because the drawn ends are usually cuts, not slab edges: set cover.sides at a real slab edge" },
        depth: OptLenPos => { def: "from profile", desc: "overall depth of the flutes" },
        pitch: OptLenPos => { def: "from profile", desc: "center-to-center spacing of the ribs" },
        rib_bottom: OptLenPos => { def: "from profile", desc: "width of a rib's bottom flat (what bears on the support)" },
        rib_top: OptLenPos => { def: "from profile", desc: "width of the rib opening at the top of the deck; the top flat is pitch - rib_top" },
    }
}

const PROFILES: [(&[&str], f64, f64, f64, f64); 8] = [
    (&["1.5B", "B"], 1.5, 6.0, 1.75, 2.5),
    (&["1.5F", "F"], 1.5, 6.0, 0.5, 1.75),
    (&["1.5A", "A"], 1.5, 6.0, 0.375, 1.0),
    (&["3N", "N"], 3.0, 8.0, 1.5, 2.625),
    (&["2C", "2VLI", "2W"], 2.0, 12.0, 5.0, 7.0),
    (&["3C", "3VLI", "3W"], 3.0, 12.0, 4.75, 7.25),
    (&["1.5VLI", "1.5C", "1.5VL"], 1.5, 6.0, 3.5, 4.25),
    (&["9/16", "0.6C", "9/16C"], 0.5625, 2.5, 0.75, 1.75),
];

fn interp(p: V2, q: V2, x: f64) -> V2 {
    if (q.x - p.x).abs() < 1e-12 { V2::init(x, p.y.max(q.y)) } else { p.lerp(q, (x - p.x) / (q.x - p.x)) }
}

/// The part of an x-monotonic polyline within x0..x1.
fn window(path: &[V2], x0: f64, x1: f64) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    for w in path.windows(2) {
        let (p, q) = (w[0], w[1]);
        if q.x < x0 || p.x > x1 {
            continue;
        }
        let a = if p.x < x0 { interp(p, q, x0) } else { p };
        let b = if q.x > x1 { interp(p, q, x1) } else { q };
        for v in [a, b] {
            if !out.last().is_some_and(|l| l.eql(v, 1e-9)) {
                out.push(v);
            }
        }
    }
    out
}

fn offset_up(path: &[V2], t: f64) -> Vec<V2> {
    let normal = |a: V2, b: V2| b.sub(a).norm().perp();
    (0..path.len())
        .map(|i| {
            let n0 = if i > 0 { normal(path[i - 1], path[i]) } else { normal(path[0], path[1]) };
            let n1 = if i + 1 < path.len() { normal(path[i], path[i + 1]) } else { n0 };
            let k = t / (1.0 + n0.dot(n1)).max(0.2);
            path[i].add(n0.add(n1).scale(k))
        })
        .collect()
}

fn pts(vs: impl IntoIterator<Item = V2>) -> Vec<Pt> {
    vs.into_iter().map(|v| Pt::new(v.x, v.y)).collect()
}

pub fn build(ctx: &mut Ctx) -> Option<Built> {
    let dp = ctx.p.parse::<Params>()?;
    let key = dp.profile.trim().to_ascii_uppercase();
    let table = PROFILES.iter().find(|p| p.0.iter().any(|n| *n == key));
    let custom = [dp.depth, dp.pitch, dp.rib_bottom, dp.rib_top];
    let (depth, pitch, rb, rt) = match (table, custom) {
        (Some(&(_, d, p, b, t)), _) => {
            (dp.depth.unwrap_or(d), dp.pitch.unwrap_or(p), dp.rib_bottom.unwrap_or(b), dp.rib_top.unwrap_or(t))
        }
        (None, [Some(d), Some(p), Some(b), Some(t)]) => (d, p, b, t),
        _ => {
            ctx.p.fail("profile", format!("metal_deck profile \"{}\" is not in the table. Profiles: 1.5B, 1.5F, 1.5A, 3N, 2C, 3C, 1.5VLI, 9/16 (also B, F, A, N, 2VLI, 3VLI, 2W, 3W, 0.6C). For another deck keep the name and give all of \"depth\", \"pitch\", \"rib_bottom\" and \"rib_top\" (inches)", dp.profile));
            return None;
        }
    };
    let Some(t) = gauge_thickness(u32::from(dp.gauge)) else {
        ctx.p.fail(
            "gauge",
            format!(
                "gauge {} is not a deck gauge: use 16, 18, 20, 22 (roof and composite deck) or 24, 26, 28 (form deck)",
                dp.gauge
            ),
        );
        return None;
    };
    if rt < rb || rt >= pitch || depth <= 4.0 * t {
        ctx.p.fail("profile", format!("deck profile does not close: need rib_bottom {} <= rib_top {} < pitch {} and a depth {} over the sheet thickness", fmt_ft_in(rb), fmt_ft_in(rt), fmt_ft_in(pitch), fmt_ft_in(depth)));
        return None;
    }
    for m in ["steel", dp.topping_material.as_str()] {
        if !material_ok(ctx, if m == "steel" { "profile" } else { "topping_material" }, m) {
            return None;
        }
    }
    if dp.topping.is_none() && (ctx.p.has("cover") || ctx.p.has("topping_material")) {
        let k = if ctx.p.has("cover") { "cover" } else { "topping_material" };
        ctx.p.fail(
            k,
            format!(
                "'{k}' needs \"topping\": the concrete thickness over the top of the flutes, e.g. \"topping\": 2.5"
            ),
        );
        return None;
    }
    let cov = parse_cover(ctx, "cover", Cover { bottom: 0.75, sides: 0.0, top: 0.75 })?;
    let mut until_note = String::new();
    let l = length_or_until(
        ctx,
        "x",
        "a metal_deck needs its extent along x, e.g. \"length\": 36, or \"until\": \"other@anchor\"",
        &mut until_note,
    )?;
    if !ctx.p.ok {
        return None;
    }
    let steel = |lp: Vec<Pt>| Prism {
        part: "deck".to_string(),
        material: "steel".to_string(),
        loops: model::one_loop(oriented_ccw(lp)),
        ..Prism::default()
    };
    let top = dp.topping.map(|c| depth + c);
    let mut prisms: Vec<Prism> = Vec::new();
    let mut anchors: Vec<NamedAnchor> = Vec::new();
    let mut add = |n: String, x: f64, y: f64| anchors.push(NamedAnchor { name: n, p: V2::init(x, y) });
    let mut host = None;
    if dp.run == Run::Z {
        let h = depth - t;
        let first = pitch / 2.0 + dp.offset - pitch * crate::math::ceil((pitch / 2.0 + dp.offset + rt) / pitch);
        let n = crate::num::to_int_clamped::<usize>(crate::math::ceil((l - first + rt) / pitch) + 1.0, 1, 100_000);
        let mut lower: Vec<V2> = Vec::new();
        let (mut ribs, mut flats) = (Vec::new(), Vec::new());
        for k in 0..n {
            let c = first + k as f64 * pitch;
            lower.extend([
                V2::init(c - rt / 2.0, h),
                V2::init(c - rb / 2.0, 0.0),
                V2::init(c + rb / 2.0, 0.0),
                V2::init(c + rt / 2.0, h),
            ]);
            if c > 1e-6 && c < l - 1e-6 {
                ribs.push(c);
            }
            let f = c + pitch / 2.0;
            if f > 1e-6 && f < l - 1e-6 {
                flats.push(f);
            }
        }
        let upper = window(&offset_up(&lower, t), 0.0, l);
        let lower = window(&lower, 0.0, l);
        if lower.len() < 2 || upper.len() < 2 {
            ctx.p.fail("length", format!("length {} is too short to draw the deck profile", fmt_ft_in(l)));
            return None;
        }
        let mut sheet = lower.clone();
        sheet.extend(upper.iter().rev().copied());
        prisms.push(steel(pts(sheet)));
        if let Some(y) = top {
            let mut lp = upper.clone();
            lp.extend([V2::init(l, y), V2::init(0.0, y)]);
            let lp = oriented_ccw(pts(lp));
            host = Some(HostInfo { outline: lp.clone(), cover: cov.cover, part_cover: cov.parts, cover_set: cov.set });
            prisms.push(Prism {
                part: "topping".to_string(),
                material: dp.topping_material.clone(),
                loops: model::one_loop(lp),
                ..Prism::default()
            });
        }
        for (k, x) in ribs.iter().enumerate() {
            add(format!("rib_{}", k + 1), *x, 0.0);
        }
        for (k, x) in flats.iter().enumerate() {
            add(format!("flat_{}", k + 1), *x, depth);
        }
    } else {
        prisms.push(steel(model::rect_loop(0.0, 0.0, l, t)));
        prisms.push(Prism { embedded: top.is_some(), ..steel(model::rect_loop(0.0, depth - t, l, depth)) });
        if let Some(y) = top {
            let lp = model::rect_loop(0.0, t, l, y);
            host = Some(HostInfo { outline: lp.clone(), cover: cov.cover, part_cover: cov.parts, cover_set: cov.set });
            prisms.push(Prism {
                part: "topping".to_string(),
                material: dp.topping_material.clone(),
                loops: model::one_loop(lp),
                ..Prism::default()
            });
        }
    }
    add("deck_top".into(), l / 2.0, depth);
    add("deck_bottom".into(), l / 2.0, 0.0);
    if let Some(y) = top {
        add("topping_top".into(), l / 2.0, y);
        add("topping_top_left".into(), 0.0, y);
        add("topping_top_right".into(), l, y);
    }
    let slab = dp.topping.map_or(String::new(), |c| format!(" + {} {} topping", fmt_ft_in(c), dp.topping_material));
    Some(Built {
        prisms,
        anchors,
        box_: Box { x0: 0.0, y0: 0.0, x1: l, y1: depth },
        host,
        info: format!(
            "metal deck {} {} ga. ({} deep, ribs @ {}) flutes along {} L={}{until_note}{slab}",
            dp.profile.trim(),
            dp.gauge,
            fmt_ft_in(depth),
            fmt_ft_in(pitch),
            dp.run.tag(),
            fmt_ft_in(l)
        ),
        ..Built::default()
    })
}
