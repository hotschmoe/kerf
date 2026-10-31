//! Framing hardware checks (SPEC 27.2) once every component is placed: the builders' own findings (W_UNKNOWN_MODEL,
//! W_HANGER_SIZE, W_HOLDOWN_ROD), a post base that bears on nothing (W_POST_BASE) and a holdown rod anchored in nothing
//! (W_HOLDOWN_ROD).

use crate::clip;
use crate::geom::{self, FLAT_TOL, V2};
use crate::model::{Bears, Diags, Kind, Level};
use crate::scene::{Comp, Scene, State};
use crate::style::Role;
use crate::units::fmt_ft_in;
use alloc::format;
use alloc::string::String;

pub fn run(scene: &Scene<'_>, diags: &Diags) {
    for c in scene.comps.iter().filter(|c| c.state == State::Ok) {
        for d in &c.built.hw.warns {
            if !diags.list().contains(d) {
                diags.push(d.clone());
            }
        }
    }
}

/// The checks against the members under or around a hardware piece, over the components one or more views draw
/// (SPEC 9, 28.1).
pub fn drawn(comps: &[&Comp<'_>], diags: &Diags) {
    for c in comps {
        if let Some(b) = &c.built.hw.bears {
            bearing(comps, c, b, diags);
        }
        if let Some(r) = &c.built.hw.rod {
            rod_anchor(comps, c, r, diags);
        }
    }
}

fn plan_overlap(p: &crate::model::Prism, x: [f64; 2], z: [f64; 2]) -> Option<geom::Box> {
    let b = clip::loops_box(&clip::from_region(&p.loops, FLAT_TOL));
    let ok = b.x1.min(x[1]) - b.x0.max(x[0]) > 1e-6 && p.z1.min(z[1]) - p.z0.max(z[0]) > 1e-6;
    ok.then_some(b)
}

fn bearing(comps: &[&Comp<'_>], c: &Comp<'_>, b: &Bears, diags: &Diags) {
    let tol = 1.0 / 32.0;
    let (x, z) = ([b.plan[0], b.plan[1]], [b.plan[2], b.plan[3]]);
    let mut below: Option<(f64, &str)> = None;
    for o in comps.iter().filter(|o| o.index != c.index && !b.post.contains(&o.id) && !o.ty.traits.hardware) {
        for p in o.world.iter().filter(|p| p.kind == Kind::Body && p.role != Role::Soil && !p.embedded) {
            let Some(pb) = plan_overlap(p, x, z) else { continue };
            if (pb.y1 - b.y).abs() <= tol {
                return;
            }
            if pb.y1 < b.y + tol + b.standoff.max(0.0) + 0.5
                && pb.y1 > b.y - 12.0
                && below.is_none_or(|bb| pb.y1 > bb.0)
            {
                below = Some((pb.y1, o.id.as_str()));
            }
        }
    }
    let id = &c.id;
    let (msg, fix) = match below {
        Some((top, host)) => {
            let so = b.post_y0 - top;
            let fix = if so >= 0.0 {
                format!(
                    "make the standoff match: {{\"op\":\"update\",\"path\":\"components/{id}\",\"value\":{{\"standoff\":{}}}}}, or move '{}' so its bottom is at y {} (the top of '{host}' plus the standoff)",
                    crate::model::fmt_num(crate::math::round(so * 10000.0) / 10000.0),
                    b.post.join("+"),
                    fmt_ft_in(top + b.standoff)
                )
            } else {
                format!(
                    "'{}' runs into '{host}': move it up so its bottom is at y {}",
                    b.post.join("+"),
                    fmt_ft_in(top + b.standoff)
                )
            };
            (
                format!(
                    "post base '{id}' bears on nothing: its plate bottom is at y {} (post '{}' bottom {} less the standoff {}), but the top of '{host}' under it is at y {}",
                    fmt_ft_in(b.y),
                    b.post.join("+"),
                    fmt_ft_in(b.post_y0),
                    fmt_ft_in(b.standoff),
                    fmt_ft_in(top)
                ),
                fix,
            )
        }
        None => (
            format!("post base '{id}' bears on nothing: no member under its plate (bottom at y {})", fmt_ft_in(b.y)),
            String::from("place the footing, slab or pier under the post, or remove the post base"),
        ),
    };
    diags.add_fix(Level::Warning, "W_POST_BASE", Some(id), Some(&format!("components/{id}/standoff")), msg, fix);
}

fn rod_anchor(comps: &[&Comp<'_>], c: &Comp<'_>, rod: &str, diags: &Diags) {
    let Some(r) = comps.iter().find(|r| r.id == rod) else { return };
    let mut bx = geom::Box::default();
    let (mut z0, mut z1) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in r.world.iter().filter(|p| p.instance == 0 && p.part == "shank") {
        bx.add_box(clip::loops_box(&clip::from_region(&p.loops, FLAT_TOL)));
        z0 = z0.min(p.z0);
        z1 = z1.max(p.z1);
    }
    if bx.is_empty() {
        return;
    }
    let tip = V2::init((bx.x0 + bx.x1) / 2.0, bx.y0 + 0.25);
    let zc = (z0 + z1) / 2.0;
    let anchored = comps.iter().filter(|o| o.index != r.index && !o.ty.traits.hardware).any(|o| {
            o.world.iter().any(|p| {
                p.kind == Kind::Body
                    && (p.role.is_masonry() || p.role == Role::Steel)
                    && zc >= p.z0
                    && zc <= p.z1
                    && geom::locate(tip, &clip::from_region(&p.loops, FLAT_TOL), 1e-6) != geom::Loc::Outside
            })
        });
    if anchored {
        return;
    }
    let id = &c.id;
    diags.add_fix(
        Level::Warning,
        "W_HOLDOWN_ROD",
        Some(id),
        Some(&format!("components/{id}/rod")),
        format!(
            "holdown '{id}': rod '{rod}' ends at y {} in no concrete, masonry or steel member, so the uplift it takes has nowhere to go",
            fmt_ft_in(bx.y0)
        ),
        format!(
            "embed '{rod}' in the foundation (an anchor_bolt placed at the concrete top with its embed), or run it through a steel member"
        ),
    );
}
