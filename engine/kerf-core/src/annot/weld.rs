//! Weld symbols (AWS A2.4 basics): an arrow to the joint, a horizontal reference line, the weld symbol below the line for
//! the arrow side and above it for the other side (fillet triangle with its vertical leg on the left; bevel groove for CJP and
//! PJP), the size left of the symbol, an all-around circle and a field-weld flag at the elbow, and a tail with its text.
//! Placement is explicit (`at` + `offset`): the symbol is not moved by the note layout. Section views only.

use super::text::{path_item, text_item, upper_if};
use super::{Env, non_null, str_member};
use crate::drawing::{Align, FillItem, Item, VAlign};
use crate::geom::{Pt, V2};
use crate::json::Value;
use crate::model::{self, Level};
use crate::pen::{LayerKey, Pen};
use crate::units;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const KINDS: [&str; 3] = ["fillet", "cjp", "pjp"];
const SIDES: [&str; 3] = ["arrow", "other", "both"];

pub struct WeldSpec<'a> {
    pub id: &'a str,
    pub at: V2,
    pub elbow: V2,
    pub kind: &'static str,
    pub size: Option<f64>,
    pub sides: &'static str,
    pub all_around: bool,
    pub field: bool,
    pub tail: Option<&'a str>,
}

fn choice(env: &Env, an: &Value, id: &str, apath: &str, key: &str, allowed: &[&'static str]) -> Option<&'static str> {
    let Some(v) = non_null(an, key) else { return allowed.first().copied() };
    if let Some(s) = v.str()
        && let Some(c) = allowed.iter().find(|c| **c == s)
    {
        return Some(c);
    }
    env.diags.add(
        Level::Error,
        "E_PARAM",
        Some(id),
        Some(&format!("{apath}/{key}")),
        format!("weld '{id}': {key} must be one of {} (got {})", model::join_quoted(allowed), model::kind_or_text(v)),
    );
    None
}

fn flag(env: &Env, an: &Value, id: &str, apath: &str, key: &str) -> Option<bool> {
    match non_null(an, key) {
        None => Some(false),
        Some(Value::Bool(b)) => Some(*b),
        Some(v) => {
            env.diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/{key}")),
                format!("weld '{id}': {key} must be true or false (got {})", model::kind_or_text(v)),
            );
            None
        }
    }
}

pub fn read<'a>(env: &mut Env<'a, '_>, an: &'a Value, id: &'a str, apath: &str) -> Option<WeldSpec<'a>> {
    let Some(atv) = non_null(an, "at") else {
        env.diags.add_fix(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(apath),
            format!("weld '{id}' needs \"at\": the joint the arrow points to (a Ref or [x, y])"),
            "e.g. \"at\": \"column@bottom_right\" (the toe of the weld at the column/base plate corner)".into(),
        );
        return None;
    };
    let at = env.scene.point(atv, id, &format!("{apath}/at"))?;
    let off = match non_null(an, "offset") {
        Some(ov) => model::offset_pair_or_diag(env.diags, Some(id), &format!("{apath}/offset"), "weld offset", ov)?,
        None => V2::init(0.5 * env.s, 0.5 * env.s),
    };
    let kind = choice(env, an, id, apath, "weld", &KINDS)?;
    let sides = choice(env, an, id, apath, "sides", &SIDES)?;
    let size = match non_null(an, "size") {
        None => None,
        Some(v) => match units::parse_length(v).filter(|x| *x > 0.0) {
            Some(x) => Some(x),
            None => {
                env.diags.add(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("{apath}/size")),
                    format!(
                        "weld '{id}': size must be a positive length such as \"3/16\" (got {})",
                        model::kind_or_text(v)
                    ),
                );
                return None;
            }
        },
    };
    if kind == "fillet" && size.is_none() {
        env.diags.add_fix(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(apath),
            format!("weld '{id}': a fillet weld needs \"size\" (the leg size)"),
            "e.g. \"size\": \"3/16\"".into(),
        );
        return None;
    }
    let all_around = flag(env, an, id, apath, "all_around")?;
    let field = flag(env, an, id, apath, "field")?;
    Some(WeldSpec { id, at, elbow: at.add(off), kind, size, sides, all_around, field, tail: str_member(an, "tail") })
}

/// Size text: sixteenths as a fraction (`3/16`, `5/16`, `1/2`), else the decimal.
pub fn size_text(x: f64) -> String {
    let t = units::fmt_ft_in(x);
    String::from(t.trim_end_matches('"'))
}

fn fill(env: &Env, id: &str, pts: &[V2]) -> Item {
    Item::Fill(FillItem {
        layer: String::from(env.style.layer_name(LayerKey::Notes)),
        src: String::from(id),
        loops: alloc::vec![pts.iter().map(|&p| Pt::at(p, 0.0)).collect()],
    })
}

pub fn items(env: &mut Env, w: &WeldSpec) -> Vec<Item> {
    let s = env.s;
    let st = env.style;
    let h = st.text_height_in * s;
    let a = st.text_height_in * s;
    let gap = 0.04 * s;
    let dir = if w.elbow.x >= w.at.x { 1.0 } else { -1.0 };
    let size = w.size.map(size_text);
    let size_w = size.as_deref().map_or(0.0, |t| env.font.width(t, h));
    let tail = w.tail.map(|t| upper_if(env, t).into_owned()).or(match w.kind {
        "cjp" => Some(String::from("CJP")),
        "pjp" => Some(String::from("PJP")),
        _ => None,
    });
    let reach = size_w + gap + a + 0.25 * s;
    let len = (0.55 * s).max(reach + 0.15 * s);
    let e = w.elbow;
    let end = V2::init(e.x + dir * len, e.y);
    let mut out: Vec<Item> = Vec::new();
    let d = w.at.sub(e).norm();
    let alen = st.arrow_len_in * s;
    let aw = st.arrow_width_in * s;
    let base = w.at.sub(d.scale(alen));
    out.push(path_item(env, Pen::Anno, w.id, &[e, base], false));
    out.push(fill(env, w.id, &[w.at, base.add(d.perp().scale(aw)), base.sub(d.perp().scale(aw))]));
    out.push(path_item(env, Pen::Anno, w.id, &[e, end], false));
    let left_x = e.x.min(end.x);
    let sx = left_x + 0.2 * s + size_w + gap;
    let mut sides: Vec<f64> = Vec::new();
    if w.sides != "other" {
        sides.push(-1.0);
    }
    if w.sides != "arrow" {
        sides.push(1.0);
    }
    for sg in sides {
        let y = e.y;
        let tip = V2::init(sx, y + sg * a);
        if w.kind == "fillet" {
            out.push(path_item(env, Pen::Anno, w.id, &[V2::init(sx, y), tip, V2::init(sx + a, y)], false));
        } else {
            out.push(path_item(env, Pen::Anno, w.id, &[V2::init(sx, y), tip], false));
            out.push(path_item(env, Pen::Anno, w.id, &[V2::init(sx, y), V2::init(sx + a, y + sg * a)], false));
        }
        if let Some(t) = &size {
            let (ty, va) =
                if sg < 0.0 { (y - 0.03 * s - h, VAlign::Baseline) } else { (y + 0.03 * s, VAlign::Baseline) };
            out.push(text_item(env, LayerKey::Notes, Pen::Anno, w.id, t, sx - gap, ty, h, 0.0, Align::Right, va));
        }
    }
    if w.all_around {
        let r = 0.04 * s;
        out.push(Item::Path(crate::drawing::PathItem {
            layer: String::from(st.layer_for_pen(Pen::Anno)),
            pen: Pen::Anno,
            src: String::from(w.id),
            closed: true,
            pts: model::circle_loop(e.x, e.y, r),
        }));
    }
    if w.field {
        let top = e.y + 0.16 * s;
        out.push(path_item(env, Pen::Anno, w.id, &[e, V2::init(e.x, top)], false));
        let fl = 0.07 * s;
        out.push(fill(
            env,
            w.id,
            &[V2::init(e.x, top), V2::init(e.x, top - fl), V2::init(e.x + dir * 1.3 * fl, top - fl / 2.0)],
        ));
    }
    if let Some(t) = tail {
        let k = 0.07 * s;
        out.push(path_item(
            env,
            Pen::Anno,
            w.id,
            &[V2::init(end.x + dir * k, end.y + k), end, V2::init(end.x + dir * k, end.y - k)],
            false,
        ));
        let align = if dir > 0.0 { Align::Left } else { Align::Right };
        out.push(text_item(
            env,
            LayerKey::Notes,
            Pen::Anno,
            w.id,
            &t,
            end.x + dir * (k + gap),
            end.y,
            h,
            0.0,
            align,
            VAlign::Middle,
        ));
    }
    out
}
