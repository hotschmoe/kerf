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
    /// Layouts (SPEC 26.2): a title wider than this (model units) wraps at spaces; 0 keeps one line.
    pub wrap_w: f64,
}

/// The title in lines no wider than `w` (a word wider than `w` keeps its own line).
fn title_lines(env: &Env, title: &str, h: f64, w: f64) -> Vec<String> {
    if w <= 0.0 || env.font.width(ascii_fold(title), h) <= w {
        return alloc::vec![String::from(title)];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in title.split(' ').filter(|x| !x.is_empty()) {
        let trial = if cur.is_empty() { String::from(word) } else { format!("{cur} {word}") };
        if !cur.is_empty() && env.font.width(ascii_fold(&trial), h) > w {
            lines.push(core::mem::replace(&mut cur, String::from(word)));
        } else {
            cur = trial;
        }
    }
    lines.push(cur);
    lines
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
    let mut ty = cy + 0.02 * sc;
    let lines = title_lines(env, &title, th, info.wrap_w);
    let mut tw: f64 = 0.0;
    for (k, line) in lines.iter().enumerate() {
        if k > 0 {
            ty -= th * 1.4;
        }
        out.push(text_item(
            env,
            LayerKey::Title,
            Pen::Title,
            &src,
            line,
            tx,
            ty,
            th,
            0.0,
            Align::Left,
            VAlign::Baseline,
        ));
        tw = tw.max(env.font.width(ascii_fold(line), th));
    }
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
