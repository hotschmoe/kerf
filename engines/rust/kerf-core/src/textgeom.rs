//! Stroke-font text to model-space polylines (shared by SVG, PDF and the rasterizer).

use crate::drawing::{Align, TextItem, VAlign};
use crate::font::Font;
use crate::geom::V2;
use crate::math;
use alloc::vec::Vec;

/// Polylines for a text item (anchor at item.x, item.y; rotation about the anchor, degrees CCW).
pub fn strokes(font: &Font, t: &TextItem) -> Vec<Vec<V2>> {
    let mut list = Vec::new();
    font.strokes_of(&mut list, &t.s, t.h);
    let w = font.width(&t.s, t.h);
    let dx = match t.align {
        Align::Left => 0.0,
        Align::Center => -w / 2.0,
        Align::Right => -w,
    };
    let dy = match t.valign {
        VAlign::Baseline => 0.0,
        VAlign::Middle => -t.h / 2.0,
        VAlign::Top => -t.h,
    };
    let r = t.rot * (math::PI / 180.0);
    let c = math::cos(r);
    let s = math::sin(r);
    for pl in &mut list {
        for p in pl.iter_mut() {
            let x = p.x + dx;
            let y = p.y + dy;
            *p = V2::init(t.x + x * c - y * s, t.y + x * s + y * c);
        }
    }
    list
}
