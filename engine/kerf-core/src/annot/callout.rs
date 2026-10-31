//! Reference bubbles (SPEC 26.2): a detail callout, the detail number over the sheet number in a split circle, with an
//! optional leader to the point it refers to and an optional text beside it (`SEE`, `SIM.`). Placed exactly where the
//! author puts it, like a weld symbol: the note layout does not move it.

use super::text::{path_item, text_item, upper_if};
use super::{Env, at_drawn, non_null};
use crate::drawing::{Align, FillItem, Item, PathItem, VAlign};
use crate::geom::{Pt, V2};
use crate::json::Value;
use crate::model::{self, Level};
use crate::pen::{LayerKey, Pen};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct RefSpec<'a> {
    pub id: &'a str,
    pub detail: String,
    pub sheet: String,
    pub text: Option<&'a str>,
    /// Bubble centre (drawing space).
    pub center: V2,
    /// Where the leader points (drawing space); None draws no leader.
    pub at: Option<V2>,
}

/// Default bubble position from the leader point: 3/4" (paper) up and right.
const AUTO_PLACE_IN: f64 = 0.75;

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(model::num_text(*n)),
        _ => None,
    }
}

pub fn read<'a>(env: &mut Env<'a, '_>, an: &'a Value, id: &'a str, apath: &str) -> Option<RefSpec<'a>> {
    let vid = env.spec.id;
    let Some(detail) = non_null(an, "detail").and_then(text_of) else {
        env.diags.add_fix(
            Level::Error,
            "E_PARAM",
            Some(id),
            Some(&format!("{apath}/detail")),
            format!(
                "ref '{id}' in view {vid} needs \"detail\": the detail number printed in the top half of the bubble"
            ),
            "e.g. {\"type\":\"ref\",\"detail\":\"3\",\"sheet\":\"S-50.01\",\"at\":\"beam@top_right\"}".to_string(),
        );
        return None;
    };
    let sheet = match non_null(an, "sheet") {
        None => String::new(),
        Some(v) => match text_of(v) {
            Some(s) => s,
            None => {
                env.diags.add(
                    Level::Error,
                    "E_PARAM",
                    Some(id),
                    Some(&format!("{apath}/sheet")),
                    format!(
                        "ref '{id}' in view {vid}: \"sheet\" must be the sheet number text, e.g. \"S-50.01\" (got {})",
                        model::kind_or_text(v)
                    ),
                );
                return None;
            }
        },
    };
    let text = match non_null(an, "text") {
        None => None,
        Some(Value::String(s)) => Some(s.as_str()),
        Some(v) => {
            env.diags.add(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(&format!("{apath}/text")),
                format!("ref '{id}' in view {vid}: \"text\" must be a string (got {})", model::kind_or_text(v)),
            );
            return None;
        }
    };
    let at = match non_null(an, "at") {
        Some(atv) => {
            let p = env.scene.point(atv, id, &format!("{apath}/at"))?;
            Some(at_drawn(env, atv, "", p))
        }
        None => None,
    };
    let center = match (non_null(an, "place"), at) {
        (Some(pv), _) => model::offset_pair_or_diag(env.diags, Some(id), &format!("{apath}/place"), "place", pv)?,
        (None, Some(p)) => p.add(V2::init(AUTO_PLACE_IN * env.s, AUTO_PLACE_IN * env.s)),
        (None, None) => {
            env.diags.add_fix(
                Level::Error,
                "E_PARAM",
                Some(id),
                Some(apath),
                format!("ref '{id}' in view {vid} needs \"place\" (the bubble centre, [x, y]) or \"at\" (the point its leader points to)"),
                "add \"at\": \"<comp>@<anchor>\" (the bubble goes 3/4\" paper up and right of it) or \"place\": [x, y]".to_string(),
            );
            return None;
        }
    };
    Some(RefSpec { id, detail, sheet, text, center, at })
}

pub fn items(env: &mut Env, r: &RefSpec) -> Vec<Item> {
    let s = env.s;
    let st = env.style;
    let h = st.text_height_in * s;
    let sh = st.label_height_in * 0.9 * s;
    let wide = env.font.width(&r.detail, h).max(env.font.width(&r.sheet, sh));
    let rad = (0.25 * s).max(wide * 0.5 + 0.06 * s);
    let c = r.center;
    let mut out: Vec<Item> = Vec::new();
    out.push(Item::Path(PathItem {
        layer: String::from(st.layer_for_pen(Pen::Anno)),
        pen: Pen::Anno,
        src: String::from(r.id),
        closed: true,
        pts: alloc::vec![Pt { x: c.x - rad, y: c.y, b: 1.0 }, Pt { x: c.x + rad, y: c.y, b: 1.0 }],
    }));
    if r.sheet.is_empty() {
        out.push(text_item(
            env,
            LayerKey::Notes,
            Pen::Anno,
            r.id,
            &r.detail,
            c.x,
            c.y,
            h,
            0.0,
            Align::Center,
            VAlign::Middle,
        ));
    } else {
        out.push(path_item(env, Pen::Anno, r.id, &[V2::init(c.x - rad, c.y), V2::init(c.x + rad, c.y)], false));
        let dy = rad * 0.5;
        out.push(text_item(
            env,
            LayerKey::Notes,
            Pen::Anno,
            r.id,
            &r.detail,
            c.x,
            c.y + dy,
            h,
            0.0,
            Align::Center,
            VAlign::Middle,
        ));
        out.push(text_item(
            env,
            LayerKey::Notes,
            Pen::Anno,
            r.id,
            &r.sheet,
            c.x,
            c.y - dy,
            sh,
            0.0,
            Align::Center,
            VAlign::Middle,
        ));
    }
    if let Some(t) = r.text {
        let t = upper_if(env, t).into_owned();
        let (x, align) = match r.at {
            Some(p) if p.x > c.x + rad => (c.x - rad - 0.08 * s, Align::Right),
            _ => (c.x + rad + 0.08 * s, Align::Left),
        };
        out.push(text_item(env, LayerKey::Notes, Pen::Anno, r.id, &t, x, c.y, h, 0.0, align, VAlign::Middle));
    }
    if let Some(p) = r.at {
        let v = p.sub(c);
        let len = v.len();
        let alen = st.arrow_len_in * s;
        if len > rad + alen {
            let d = v.scale(1.0 / len);
            let start = c.add(d.scale(rad));
            let base = p.sub(d.scale(alen));
            let aw = st.arrow_width_in * s;
            out.push(path_item(env, Pen::Anno, r.id, &[start, base], false));
            out.push(Item::Fill(FillItem {
                layer: String::from(st.layer_name(LayerKey::Notes)),
                src: String::from(r.id),
                loops: alloc::vec![alloc::vec![
                    Pt::at(p, 0.0),
                    Pt::at(base.add(d.perp().scale(aw)), 0.0),
                    Pt::at(base.sub(d.perp().scale(aw)), 0.0)
                ]],
            }));
        }
    }
    out
}
