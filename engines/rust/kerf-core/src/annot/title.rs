//! Title bubble, title text and scale line under a view (SPEC 6.4).

use super::Env;
use super::text::{ascii_fold, layer_name, path_item, text_item, upper_if};
use crate::drawing::{Align, Item, PathItem, VAlign};
use crate::geom::{Box, Pt, V2};
use crate::pen::{LayerKey, Pen};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct SheetInfo<'a> {
    pub number: &'a str,
    pub title: &'a str,
    pub scale_text: &'a str,
    pub sheet: &'a str,
    pub unverified: bool,
}

/// Title under the view: bubble, title text with heavy underline, scale (SPEC 6.5). Returns the lowest y used.
pub fn title_items(env: &mut Env, info: &SheetInfo, tcrop: Box, out: &mut Vec<Item>) -> f64 {
    let st = env.style;
    let sc = env.s;
    let r = 0.3125 * sc;
    let top_gap = 0.4 * sc;
    let cx = tcrop.x0 + r;
    let cy = tcrop.y0 - top_gap - r;
    let src = format!("title:{}", env.spec.id);
    let bubble = alloc::vec![Pt { x: cx - r, y: cy, b: 1.0 }, Pt { x: cx + r, y: cy, b: 1.0 }];
    out.push(Item::Path(PathItem {
        layer: String::from(layer_name(env, LayerKey::Title)),
        pen: Pen::Title,
        src: src.clone(),
        closed: true,
        pts: bubble,
    }));
    let th = st.title_height_in * sc;
    let nh = st.text_height_in * sc;
    if info.sheet.is_empty() {
        out.push(text_item(
            env,
            LayerKey::Title,
            Pen::Title,
            &src,
            info.number,
            cx,
            cy,
            th,
            0.0,
            Align::Center,
            VAlign::Middle,
        ));
    } else {
        out.push(path_item(env, Pen::Anno, &src, &[V2::init(cx - r, cy), V2::init(cx + r, cy)], false));
        out.push(text_item(
            env,
            LayerKey::Title,
            Pen::Title,
            &src,
            info.number,
            cx,
            cy + r * 0.5,
            th,
            0.0,
            Align::Center,
            VAlign::Middle,
        ));
        out.push(text_item(
            env,
            LayerKey::Title,
            Pen::Anno,
            &src,
            info.sheet,
            cx,
            cy - r * 0.5,
            st.label_height_in * 0.9 * sc,
            0.0,
            Align::Center,
            VAlign::Middle,
        ));
    }
    let tx = cx + r + 0.15 * sc;
    let title = upper_if(env, info.title).into_owned();
    let ty = cy + 0.02 * sc;
    out.push(text_item(env, LayerKey::Title, Pen::Title, &src, &title, tx, ty, th, 0.0, Align::Left, VAlign::Baseline));
    let tw = env.font.width(ascii_fold(&title), th);
    let uy = ty - 0.07 * sc;
    out.push(path_item(env, Pen::Title, &src, &[V2::init(tx, uy), V2::init(tx + tw, uy)], false));
    let scale_line = format!("SCALE: {}", info.scale_text);
    let sy = uy - 0.06 * sc - nh;
    out.push(text_item(
        env,
        LayerKey::Title,
        Pen::Anno,
        &src,
        &scale_line,
        tx,
        sy,
        nh,
        0.0,
        Align::Left,
        VAlign::Baseline,
    ));
    let mut low = (cy - r).min(sy - 0.02 * sc);
    if info.unverified {
        let fy = low - 0.1 * sc - nh;
        out.push(text_item(
            env,
            LayerKey::Title,
            Pen::Anno,
            "footnote",
            &st.cite_footnote,
            tcrop.x0,
            fy,
            nh * 0.85,
            0.0,
            Align::Left,
            VAlign::Baseline,
        ));
        low = fy - 0.02 * sc;
    }
    low
}
