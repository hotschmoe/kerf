//! Text helpers of the annotation layout (SPEC 16): glyph folding to the stroke font's ASCII set, character-count wrapping, text boxes and the text/path items every annotation emits.

use super::Env;
use crate::drawing::{Align, Item, PathItem, TextItem, VAlign};
use crate::font::Font;
use crate::geom::{self, Box, Pt, V2};
use crate::house;
use crate::math;
use crate::pen::{LayerKey, Pen};
use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const RAD_PER_DEG: f64 = 0.017453292519943295;
pub const DEG_PER_RAD: f64 = 57.29577951308232;

/// Glyph folding (SPEC 16): dashes -> '-', smart quotes -> straight, x-sign -> X, vulgar fractions ->
/// " n/d", anything else non-ASCII -> '?'. `unknown` is set when a '?' substitution happened.
pub fn ascii_fold_flag<'s>(s: &'s str, unknown: Option<&mut bool>) -> Cow<'s, str> {
    if s.is_ascii() {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut unk = false;
    for ch in s.chars() {
        match ch as u32 {
            0x2010..=0x2015 | 0x2212 => out.push('-'),
            0xD7 => out.push('X'),
            0x201C | 0x201D | 0x201E | 0x2033 => out.push('"'),
            0x2018 | 0x2019 | 0x201A | 0x2032 => out.push('\''),
            0xA0 | 0x2009 | 0x200A | 0x202F | 0x2002 | 0x2003 => out.push(' '),
            0xBD => out.push_str(" 1/2"),
            0xBC => out.push_str(" 1/4"),
            0xBE => out.push_str(" 3/4"),
            0x215B => out.push_str(" 1/8"),
            0x215C => out.push_str(" 3/8"),
            0x215D => out.push_str(" 5/8"),
            0x215E => out.push_str(" 7/8"),
            0..=127 => out.push(ch),
            _ => {
                out.push('?');
                unk = true;
            }
        }
    }
    if unk && let Some(u) = unknown {
        *u = true;
    }
    Cow::Owned(out)
}

pub fn ascii_fold(s: &str) -> String {
    ascii_fold_flag(s, None).into_owned()
}

pub fn layer_name<'e>(env: &'e Env, key: LayerKey) -> &'e str {
    env.style.layer_name(key)
}

#[allow(clippy::too_many_arguments)]
pub fn text_item(
    env: &mut Env,
    layer_key: LayerKey,
    pen: Pen,
    src: &str,
    s: &str,
    x: f64,
    y: f64,
    h: f64,
    rot: f64,
    align: Align,
    valign: VAlign,
) -> Item {
    let layer = String::from(layer_name(env, layer_key));
    let folded = ascii_fold_flag(s, Some(&mut env.unknown_glyph)).into_owned();
    Item::Text(TextItem { layer, pen, src: String::from(src), s: folded, x, y, h, rot, align, valign })
}

pub fn path_item(env: &Env, pen: Pen, src: &str, pts: &[V2], closed: bool) -> Item {
    let pts: Vec<Pt> = pts.iter().map(|&q| Pt::at(q, 0.0)).collect();
    Item::Path(PathItem { layer: String::from(env.style.layer_for_pen(pen)), pen, src: String::from(src), closed, pts })
}

/// The style's case transform: upper case, except the tokens whose case is significant (SPEC 25.3: `16d`, `db`, `f'c`, ...).
pub fn upper_if<'s>(env: &Env, s: &'s str) -> Cow<'s, str> {
    if env.style.text_case_upper { Cow::Owned(house::upper(s, env.style)) } else { Cow::Borrowed(s) }
}

/// Text box polygon (rotated) with padding.
pub fn text_poly(font: &Font, t: &TextItem, pad: f64) -> [V2; 4] {
    let w = font.width(&t.s, t.h);
    let ox = match t.align {
        Align::Center => -w * 0.5,
        Align::Right => -w,
        Align::Left => 0.0,
    };
    let oy = match t.valign {
        VAlign::Middle => -t.h * 0.5,
        VAlign::Baseline => 0.0,
    };
    let ang = t.rot * RAD_PER_DEG;
    let c = math::cos(ang);
    let s = math::sin(ang);
    let pts =
        [[ox - pad, oy - pad], [ox + w + pad, oy - pad], [ox + w + pad, oy + t.h + pad], [ox - pad, oy + t.h + pad]];
    pts.map(|q| V2::init(t.x + q[0] * c - q[1] * s, t.y + q[0] * s + q[1] * c))
}

pub fn items_box(font: &Font, items: &[Item]) -> Box {
    let mut b = Box::default();
    for it in items {
        match it {
            Item::Path(p) if p.closed => b.add_box(geom::loop_box(&p.pts)),
            Item::Path(p) => {
                b.add_box(geom::points_box(&p.pts));
                for w in p.pts.windows(2) {
                    geom::seg_box_into(&mut b, w[0].v(), w[1].v(), w[0].b);
                }
            }
            Item::Fill(f) => {
                for l in &f.loops {
                    b.add_box(geom::points_box(l));
                }
            }
            Item::Hatch(h) => {
                for l in &h.loops {
                    b.add_box(geom::points_box(l));
                }
            }
            Item::Text(t) => {
                for p in text_poly(font, t, 0.0) {
                    b.add_point(p.x, p.y);
                }
            }
            Item::Region(_) => {}
        }
    }
    b
}

/// Qualifiers that stay on the line of the dimension before them (`24" MIN.`, `16" O.C.`).
const QUALIFIERS: [&str; 14] =
    ["MIN", "MIN.", "MAX", "MAX.", "O.C.", "OC", "TYP", "TYP.", "CLR", "CLR.", "EA.", "EMBED", "EMBED.", "LAP"];

fn is_measure(w: &str) -> bool {
    w.bytes().any(|b| b.is_ascii_digit()) && (w.ends_with('"') || w.ends_with('\''))
}

fn is_fraction(w: &str) -> bool {
    let b = w.as_bytes();
    b.first().is_some_and(u8::is_ascii_digit) && w.contains('/')
}

/// The words of `text`, with the pieces of one value kept together: a whole number and its fraction (`1 1/2"`), `@`
/// and the spacing after it, and a dimension and its qualifier (`24" MIN.`).
fn wrap_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut glue_next = false;
    for w in text.split([' ', '\t', '\r', '\n']).filter(|w| !w.is_empty()) {
        let join = match out.last() {
            Some(prev) => {
                glue_next
                    || (prev.bytes().all(|b| b.is_ascii_digit()) && is_fraction(w))
                    || (is_measure(prev) && QUALIFIERS.iter().any(|q| w.eq_ignore_ascii_case(q)))
            }
            None => false,
        };
        match out.last_mut() {
            Some(prev) if join => {
                prev.push(' ');
                prev.push_str(w);
            }
            _ => out.push(String::from(w)),
        }
        glue_next = w == "@";
    }
    out
}

/// Greedy word wrap at `chars_in` characters; words longer than a line are split. The pieces of one value stay on
/// one line when it fits (`wrap_words`).
pub fn wrap(text: &str, chars_in: usize) -> Vec<String> {
    let chars = chars_in.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let tokens = wrap_words(text);
    let words = tokens.iter().flat_map(|t| {
        let parts: Vec<&str> = if t.chars().count() > chars { t.split(' ').collect() } else { alloc::vec![t.as_str()] };
        parts
    });
    for word in words {
        let mut w = word;
        loop {
            let wl = w.chars().count();
            let cl = cur.chars().count();
            if cur.is_empty() {
                if wl > chars {
                    let idx = w.char_indices().nth(chars).map_or(w.len(), |(i, _)| i);
                    lines.push(String::from(&w[..idx]));
                    w = &w[idx..];
                    continue;
                }
                cur.push_str(w);
                break;
            } else if cl + 1 + wl <= chars {
                cur.push(' ');
                cur.push_str(w);
                break;
            }
            lines.push(core::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Greedy word wrap at a paper width: the lines of each paragraph of `text` (split at `\n`) no wider than `max_w` at
/// text height `h`. The pieces of one value stay together when they fit (`wrap_words`); a word wider than `max_w` is
/// never split and takes a line of its own (the caller reports the overflow).
pub fn wrap_width(font: &Font, text: &str, h: f64, max_w: f64) -> Vec<String> {
    let width = |s: &str| font.width(ascii_fold(s), h);
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        let tokens = wrap_words(para);
        let words = tokens.iter().flat_map(|t| {
            let parts: Vec<&str> = if width(t) > max_w { t.split(' ').collect() } else { alloc::vec![t.as_str()] };
            parts
        });
        for w in words {
            if cur.is_empty() {
                cur.push_str(w);
                continue;
            }
            let joined = format!("{cur} {w}");
            if width(&joined) <= max_w + 1e-9 {
                cur = joined;
            } else {
                lines.push(core::mem::replace(&mut cur, String::from(w)));
            }
        }
        lines.push(cur);
    }
    lines
}
