//! Stroke font (Hershey Simplex derivative, SPEC 15) with metrics and glyph rendering.

use crate::geom::V2;
use crate::json::{self, Value};
use crate::math;
use alloc::vec::Vec;
use core::f64::consts::PI;

pub const EMBEDDED: &str = include_str!("../../../../spec/fonts/kerf-simplex.json");

#[derive(Clone, Debug, Default)]
pub struct Glyph {
    pub adv: f64,
    pub strokes: Vec<Vec<V2>>,
}

/// The font JSON is malformed (never for `EMBEDDED`).
#[derive(Debug)]
pub struct BadFont;

#[derive(Clone, Debug)]
pub struct Font {
    pub cap: f64,
    /// Printable ASCII 32..126.
    pub ascii: Vec<Glyph>,
    /// Synthetic glyphs for a few typographic characters used in titles and notes.
    pub emdash: Glyph,
    pub endash: Glyph,
    pub degree: Glyph,
    pub times: Glyph,
    pub plusminus: Glyph,
}

fn poly(pts: &[(f64, f64)]) -> Vec<V2> {
    pts.iter().map(|&(x, y)| V2::init(x, y)).collect()
}

fn parse_glyph(v: &Value) -> Result<Glyph, BadFont> {
    let adv = v.get("adv").and_then(Value::num).ok_or(BadFont)?;
    let st = v.get("strokes").and_then(Value::arr).ok_or(BadFont)?;
    let mut strokes = Vec::with_capacity(st.len());
    for s in st {
        let pts = s.arr().ok_or(BadFont)?;
        let mut pp = Vec::with_capacity(pts.len());
        for p in pts {
            let xy = p.arr().ok_or(BadFont)?;
            let [x, y, ..] = xy else { return Err(BadFont) };
            pp.push(V2::init(x.num().unwrap_or(0.0), y.num().unwrap_or(0.0)));
        }
        strokes.push(pp);
    }
    Ok(Glyph { adv, strokes })
}

impl Font {
    pub fn parse(src: &str) -> Result<Font, BadFont> {
        let root = json::parse(src.as_bytes()).map_err(|_| BadFont)?;
        let cap = root.get("cap_height").and_then(Value::num).ok_or(BadFont)?;
        let gl = root.get("glyphs").ok_or(BadFont)?;
        let mut ascii: Vec<Glyph> = Vec::with_capacity(95);
        let mut question: Option<Glyph> = None;
        for i in 0..95u8 {
            let ch = 32 + i;
            let key = [ch];
            let slot = match core::str::from_utf8(&key).ok().and_then(|k| gl.get(k)) {
                Some(gv) => {
                    let g = parse_glyph(gv)?;
                    if ch == b'?' {
                        question = Some(g.clone());
                    }
                    g
                }
                None => Glyph::default(),
            };
            ascii.push(slot);
        }
        for (i, slot) in ascii.iter_mut().enumerate() {
            if slot.adv == 0.0 && i != 0 {
                if let Some(q) = &question {
                    *slot = q.clone();
                }
            }
        }
        let times = ascii[(b'X' - 32) as usize].clone();
        let emdash = Glyph { adv: 24.0, strokes: Vec::from([poly(&[(0.0, 8.0), (24.0, 8.0)])]) };
        let endash = Glyph { adv: 20.0, strokes: Vec::from([poly(&[(0.0, 8.0), (18.0, 8.0)])]) };
        let plusminus = Glyph {
            adv: 20.0,
            strokes: Vec::from([
                poly(&[(10.0, 17.0), (10.0, 5.0)]),
                poly(&[(4.0, 11.0), (16.0, 11.0)]),
                poly(&[(4.0, 0.0), (16.0, 0.0)]),
            ]),
        };
        // degree: a small octagon at the top.
        let deg: Vec<V2> = (0..9)
            .map(|i| {
                let ang = (i as f64) * PI / 4.0;
                V2::init(5.0 + 3.2 * math::cos(ang), 17.5 + 3.2 * math::sin(ang))
            })
            .collect();
        let degree = Glyph { adv: 12.0, strokes: Vec::from([deg]) };
        Ok(Font { cap, ascii, emdash, endash, degree, times, plusminus })
    }

    fn ascii_at(&self, c: u8) -> &Glyph {
        &self.ascii[(c - 32) as usize]
    }

    pub fn glyph(&self, cp: u32) -> &Glyph {
        match cp {
            32..=126 => &self.ascii[(cp - 32) as usize],
            0x2014 => &self.emdash,
            0x2013 => &self.endash,
            0xB0 => &self.degree,
            0xD7 => &self.times,
            0xB1 => &self.plusminus,
            0x201C | 0x201D => self.ascii_at(b'"'),
            0x2018 | 0x2019 => self.ascii_at(b'\''),
            0xA0 => &self.ascii[0],
            _ => self.ascii_at(b'?'),
        }
    }

    /// Mean advance of A-Z in font units; the reference character width for note wrapping.
    pub fn mean_cap_advance(&self) -> f64 {
        let mut s = 0.0;
        for c in b'A'..=b'Z' {
            s += self.ascii_at(c).adv;
        }
        s / 26.0
    }

    /// Advance width of `text` at cap height `h`.
    pub fn width(&self, text: impl AsRef<[u8]>, h: f64) -> f64 {
        self.width_units(text) * h / self.cap
    }

    pub fn width_units(&self, text: impl AsRef<[u8]>) -> f64 {
        let mut w = 0.0;
        for cp in utf8_iter(text.as_ref()) {
            w += self.glyph(cp).adv;
        }
        w
    }

    /// Append the stroke polylines of `text` (baseline-left at the origin, y up) scaled to cap height `h`.
    pub fn strokes_of(&self, out: &mut Vec<Vec<V2>>, text: impl AsRef<[u8]>, h: f64) {
        let k = h / self.cap;
        let mut x = 0.0;
        for cp in utf8_iter(text.as_ref()) {
            let g = self.glyph(cp);
            for s in &g.strokes {
                out.push(s.iter().map(|p| V2::init((x + p.x) * k, p.y * k)).collect());
            }
            x += g.adv;
        }
    }
}

/// Code points of a byte string; every malformed sequence yields `?` (one byte consumed, or the whole truncated tail).
pub struct Utf8Iter<'a> {
    s: &'a [u8],
    i: usize,
}

impl Iterator for Utf8Iter<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let c = *self.s.get(self.i)?;
        let n = match c {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => {
                self.i += 1;
                return Some('?' as u32);
            }
        };
        if self.i + n > self.s.len() {
            self.i = self.s.len();
            return Some('?' as u32);
        }
        match decode(&self.s[self.i..self.i + n]) {
            Some(cp) => {
                self.i += n;
                Some(cp)
            }
            None => {
                self.i += 1;
                Some('?' as u32)
            }
        }
    }
}

fn decode(b: &[u8]) -> Option<u32> {
    let cont = |x: u8| (x & 0xC0 == 0x80).then_some((x & 0x3F) as u32);
    match *b {
        [a] => Some(a as u32),
        [a, b1] => {
            let cp = ((a & 0x1F) as u32) << 6 | cont(b1)?;
            (cp >= 0x80).then_some(cp)
        }
        [a, b1, b2] => {
            let cp = ((a & 0x0F) as u32) << 12 | cont(b1)? << 6 | cont(b2)?;
            (cp >= 0x800 && !(0xD800..=0xDFFF).contains(&cp)).then_some(cp)
        }
        [a, b1, b2, b3] => {
            let cp = ((a & 0x07) as u32) << 18 | cont(b1)? << 12 | cont(b2)? << 6 | cont(b3)?;
            (0x10000..=0x10FFFF).contains(&cp).then_some(cp)
        }
        _ => None,
    }
}

pub fn utf8_iter(s: &[u8]) -> Utf8Iter<'_> {
    Utf8Iter { s, i: 0 }
}

/// ASCII upper-casing (non-ASCII bytes are untouched).
pub fn upper_ascii(s: &[u8]) -> Vec<u8> {
    s.to_ascii_uppercase()
}
