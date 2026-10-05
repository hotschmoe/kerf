//! Stroke font (Hershey Simplex derivative, SPEC section 15), embedded at compile time.

use crate::geom::{Pt, pt};
use serde_json::Value;
use std::sync::OnceLock;

const FONT_JSON: &str = include_str!("../../../../spec/fonts/kerf-simplex.json");

pub struct Glyph {
    pub adv: f64,
    pub strokes: Vec<Vec<(f64, f64)>>,
}

pub struct Font {
    pub cap: f64,
    glyphs: Vec<Option<Glyph>>, // index = char code - 32 for 32..=126
}

static FONT: OnceLock<Font> = OnceLock::new();

pub fn font() -> &'static Font {
    FONT.get_or_init(|| Font::parse(FONT_JSON))
}

impl Font {
    fn parse(src: &str) -> Font {
        let v: Value = serde_json::from_str(src).expect("embedded font is valid JSON");
        let cap = v["cap_height"].as_f64().unwrap_or(21.0);
        let mut glyphs: Vec<Option<Glyph>> = (32..=126).map(|_| None).collect();
        if let Some(g) = v["glyphs"].as_object() {
            for (k, gv) in g {
                let mut it = k.chars();
                let (Some(c), None) = (it.next(), it.next()) else { continue };
                let code = c as u32;
                if !(32..=126).contains(&code) {
                    continue;
                }
                let adv = gv["adv"].as_f64().unwrap_or(16.0);
                let strokes = gv["strokes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|s| {
                                s.as_array()
                                    .map(|pts| pts.iter().map(|p| (p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0))).collect())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                glyphs[(code - 32) as usize] = Some(Glyph { adv, strokes });
            }
        }
        Font { cap, glyphs }
    }

    pub fn glyph(&self, c: char) -> &Glyph {
        let code = c as u32;
        let idx = if (32..=126).contains(&code) { (code - 32) as usize } else { (b'?' - 32) as usize };
        match &self.glyphs[idx] {
            Some(g) => g,
            None => self.glyphs[(b'?' - 32) as usize].as_ref().expect("font has '?'"),
        }
    }

    /// Advance width of `s` at cap height `h`.
    pub fn width(&self, s: &str, h: f64) -> f64 {
        let k = h / self.cap;
        s.chars().map(|c| self.glyph(c).adv * k).sum()
    }

    /// Stroke polylines for text. (x, y) is the anchor per align/valign; rot in degrees CCW.
    pub fn strokes(&self, s: &str, h: f64, x: f64, y: f64, rot_deg: f64, align: &str, valign: &str) -> Vec<Vec<Pt>> {
        let k = h / self.cap;
        let w = self.width(s, h);
        let ox = match align {
            "center" => -w * 0.5,
            "right" => -w,
            _ => 0.0,
        };
        let oy = match valign {
            "middle" => -h * 0.5,
            "top" => -h,
            _ => 0.0,
        };
        let ang = rot_deg.to_radians();
        let mut out = Vec::new();
        let mut cx = 0.0;
        for c in s.chars() {
            let g = self.glyph(c);
            for st in &g.strokes {
                let line: Vec<Pt> = st
                    .iter()
                    .map(|&(gx, gy)| {
                        let local = pt(ox + cx + gx * k, oy + gy * k);
                        let r = local.rot(ang);
                        pt(x + r.x, y + r.y)
                    })
                    .collect();
                out.push(line);
            }
            cx += g.adv * k;
        }
        out
    }
}

/// Glyph folding (SPEC 16): fold characters the stroke font lacks; returns the folded text and the
/// characters that had no fold and became `?`.
pub fn fold_report(s: &str) -> (String, Vec<char>) {
    let mut out = String::with_capacity(s.len());
    let mut bad = vec![];
    for c in s.chars() {
        match c {
            '\u{2014}' | '\u{2013}' | '\u{2012}' | '\u{2212}' | '\u{2010}' | '\u{2011}' => out.push('-'),
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => out.push('"'),
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => out.push('\''),
            '\u{00D7}' => out.push('X'),
            '\u{00B0}' => out.push_str(" DEG"),
            '\u{00BD}' => out.push_str(" 1/2"),
            '\u{00BC}' => out.push_str(" 1/4"),
            '\u{00BE}' => out.push_str(" 3/4"),
            '\u{215B}' => out.push_str(" 1/8"),
            '\u{215C}' => out.push_str(" 3/8"),
            '\u{215D}' => out.push_str(" 5/8"),
            '\u{215E}' => out.push_str(" 7/8"),
            '\u{00A0}' => out.push(' '),
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                out.push('?');
                if !bad.contains(&c) {
                    bad.push(c);
                }
            }
        }
    }
    (out, bad)
}

pub fn ascii_fold(s: &str) -> String {
    fold_report(s).0
}

/// Word-wrap `text` to at most `chars` characters per line (never splitting inside a word unless too long).
pub fn wrap(text: &str, chars: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let mut w: String = word.to_string();
        loop {
            let wl = w.chars().count();
            let cl = cur.chars().count();
            if cur.is_empty() {
                if wl > chars {
                    let head: String = w.chars().take(chars).collect();
                    let tail: String = w.chars().skip(chars).collect();
                    lines.push(head);
                    w = tail;
                    continue;
                }
                cur = w.clone();
                break;
            } else if cl + 1 + wl <= chars {
                cur.push(' ');
                cur.push_str(&w);
                break;
            } else {
                lines.push(std::mem::take(&mut cur));
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapping() {
        let l = wrap("2X8 PT SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.", 28);
        assert!(l.iter().all(|s| s.chars().count() <= 28), "{:?}", l);
        assert_eq!(l.join(" "), "2X8 PT SILL PLATE W/ 5/8\" DIA. ANCHOR BOLTS @ 48\" O.C.");
        assert_eq!(wrap("ABCDEFGHIJ", 4), vec!["ABCD", "EFGH", "IJ"]);
    }
    #[test]
    fn widths() {
        let f = font();
        assert!(f.width("A", 21.0) - 18.0 < 1e-9);
    }
}
