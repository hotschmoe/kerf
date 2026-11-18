use crate::geom::V2;
use crate::math;
use alloc::vec::Vec;
use core::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub adv: f64,
    strokes: &'static [&'static [[i8; 2]]],
}

include!(concat!(env!("OUT_DIR"), "/font_data.rs"));

#[derive(Debug)]
pub struct Font {
    pub cap: f64,
    ascii: &'static [Glyph; 95],
}

static EMBEDDED: Font = Font { cap: CAP, ascii: &ASCII };

pub fn embedded() -> &'static Font {
    &EMBEDDED
}

impl Font {
    fn ascii_at(&self, c: u8) -> &Glyph {
        &self.ascii[(c - 32) as usize]
    }

    pub fn glyph(&self, cp: u32) -> &Glyph {
        match cp {
            32..=126 => &self.ascii[(cp - 32) as usize],
            0x2014 => &EMDASH,
            0x2013 => &ENDASH,
            0xB0 => &DEGREE,
            0xD7 => &TIMES,
            0xB1 => &PLUSMINUS,
            0x201C | 0x201D => self.ascii_at(b'"'),
            0x2018 | 0x2019 => self.ascii_at(b'\''),
            0xA0 => &self.ascii[0],
            _ => self.ascii_at(b'?'),
        }
    }

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

    pub fn strokes_of(&self, out: &mut Vec<Vec<V2>>, text: impl AsRef<[u8]>, h: f64) {
        let k = h / self.cap;
        let mut x = 0.0;
        for cp in utf8_iter(text.as_ref()) {
            let g = self.glyph(cp);
            if cp == 0xB0 {
                out.push(
                    (0..9)
                        .map(|i| {
                            let ang = f64::from(i) * PI / 4.0;
                            let p = V2::init(5.0 + 3.2 * math::cos(ang), 17.5 + 3.2 * math::sin(ang));
                            V2::init((x + p.x) * k, p.y * k)
                        })
                        .collect(),
                );
            }
            for s in g.strokes {
                out.push(s.iter().map(|&[px, py]| V2::init((x + f64::from(px)) * k, f64::from(py) * k)).collect());
            }
            x += g.adv;
        }
    }
}

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
        if let Some(cp) = decode(&self.s[self.i..self.i + n]) {
            self.i += n;
            Some(cp)
        } else {
            self.i += 1;
            Some('?' as u32)
        }
    }
}

fn decode(b: &[u8]) -> Option<u32> {
    let cont = |x: u8| (x & 0xC0 == 0x80).then_some(u32::from(x & 0x3F));
    match *b {
        [a] => Some(u32::from(a)),
        [a, b1] => {
            let cp = u32::from(a & 0x1F) << 6 | cont(b1)?;
            (cp >= 0x80).then_some(cp)
        }
        [a, b1, b2] => {
            let cp = u32::from(a & 0x0F) << 12 | cont(b1)? << 6 | cont(b2)?;
            (cp >= 0x800 && !(0xD800..=0xDFFF).contains(&cp)).then_some(cp)
        }
        [a, b1, b2, b3] => {
            let cp = u32::from(a & 0x07) << 18 | cont(b1)? << 12 | cont(b2)? << 6 | cont(b3)?;
            (0x10000..=0x10FFFF).contains(&cp).then_some(cp)
        }
        _ => None,
    }
}

pub fn utf8_iter(s: &[u8]) -> Utf8Iter<'_> {
    Utf8Iter { s, i: 0 }
}
