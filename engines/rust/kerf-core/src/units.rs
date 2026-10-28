//! Lengths, scales and slopes (SPEC section 1 and 6.1).

use crate::json::{self, Value};
use crate::limits;
use crate::math;
use crate::num;
use alloc::format;
use alloc::string::{String, ToString};

const RAD_PER_DEG: f64 = 0.017453292519943295;

/// Decimal number in the strict grammar `[+-]? digits [. digits] ([eE] [+-]? digits)?` (at least one digit around the point).
/// It never accepts `nan`, `inf`, hex floats or `_` separators, and returns `None` when the value overflows to infinity.
/// The one float parser for every document string.
pub fn parse_finite(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let mut digits = 0;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let mut ed = 0;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            ed += 1;
        }
        if ed == 0 {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub const LENGTH_FORMS: &str = "a number of inches or a string such as 12, \"7 5/8\", \"7-5/8\\\"\", \"3'\", \"3'-4 1/2\\\"\", \"-1'-2\\\"\", \"15/32\"";

struct Scan<'a> {
    s: &'a str,
    i: usize,
}

impl Scan<'_> {
    fn peek(&self) -> u8 {
        self.s.as_bytes().get(self.i).copied().unwrap_or(0)
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), b' ' | b'\t') {
            self.i += 1;
        }
    }

    /// A run of digits with an optional single decimal point.
    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        let mut seen_dot = false;
        let mut digits = 0;
        while let Some(&c) = self.s.as_bytes().get(self.i) {
            if c.is_ascii_digit() {
                digits += 1;
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
            } else {
                break;
            }
            self.i += 1;
        }
        if digits == 0 {
            self.i = start;
            return None;
        }
        parse_finite(&self.s[start..self.i])
    }
}

/// Parse inches[.dec] | [whole sep] n/d. `sc` is positioned at the first digit. Returns inches.
fn parse_inches(sc: &mut Scan) -> Option<f64> {
    let first = sc.number()?;
    if sc.peek() == b'/' {
        sc.i += 1;
        let d = sc.number()?;
        if d == 0.0 {
            return None;
        }
        return Some(first / d);
    }
    let save = sc.i;
    if matches!(sc.peek(), b' ' | b'-') {
        sc.i += 1;
        sc.skip_spaces();
        if let Some(n) = sc.number()
            && sc.peek() == b'/'
        {
            sc.i += 1;
            let d = sc.number()?;
            if d == 0.0 {
                return None;
            }
            return Some(first + n / d);
        }
        sc.i = save;
    }
    Some(first)
}

/// Parse a length string into inches. `None` when malformed.
pub fn parse_length_str(text: &str) -> Option<f64> {
    let mut sc = Scan { s: text.trim_matches([' ', '\t']), i: 0 };
    if sc.s.is_empty() {
        return None;
    }
    let mut neg = false;
    if matches!(sc.peek(), b'-' | b'+') {
        neg = sc.peek() == b'-';
        sc.i += 1;
        sc.skip_spaces();
    }
    let mut total = 0.0;
    let start = sc.i;
    let lead = sc.number()?;
    if sc.peek() == b'\'' {
        sc.i += 1;
        total += lead * 12.0;
        sc.skip_spaces();
        if sc.peek() == b'-' {
            sc.i += 1;
            sc.skip_spaces();
        }
        if sc.i < sc.s.len() && sc.peek() != b'"' {
            total += parse_inches(&mut sc)?;
        }
    } else {
        sc.i = start;
        total = parse_inches(&mut sc)?;
    }
    sc.skip_spaces();
    if sc.peek() == b'"' {
        sc.i += 1;
    }
    sc.skip_spaces();
    if sc.i != sc.s.len() {
        return None;
    }
    Some(if neg { -total } else { total })
}

/// A length as written, finite but not range-checked (for messages that must tell "out of range" from "not a length").
pub fn parse_length_any(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.is_finite().then_some(*n),
        Value::String(s) => parse_length_str(s),
        _ => None,
    }
}

/// True when `x` is a usable length: finite and at most `limits::MAX_COORD_IN` inches in magnitude.
pub fn length_in_range(x: f64) -> bool {
    x.abs() <= limits::MAX_COORD_IN
}

/// Parse a JSON length (number or string) in inches. `None` when it is not a length, not finite or beyond +-`MAX_COORD_IN`.
pub fn parse_length(v: &Value) -> Option<f64> {
    parse_length_any(v).filter(|x| length_in_range(*x))
}

/// Architectural feet-inches, nearest 1/16": `0"`, `7 5/8"`, `1'-0"`, `4'-1 1/2"`, `-1'-2"`.
pub fn fmt_ft_in(x: f64) -> String {
    let mut out = String::new();
    append_ft_in(&mut out, x);
    out
}

/// Alias of `fmt_ft_in` (the Zig `ftin` swallowed OOM; there is nothing to swallow here).
pub fn ftin(x: f64) -> String {
    fmt_ft_in(x)
}

pub fn append_ft_in(out: &mut String, x: f64) {
    let neg = x < 0.0;
    let Some(sixteenths) = num::to_int::<i64>(math::round(x.abs() * 16.0)) else {
        out.push_str("(out of range)");
        return;
    };
    if sixteenths == 0 {
        out.push_str("0\"");
        return;
    }
    if neg {
        out.push('-');
    }
    let feet = sixteenths / 192;
    let rem = sixteenths % 192;
    let whole = rem / 16;
    let (n, d) = reduce(rem % 16);
    if feet > 0 {
        out.push_str(&format!("{feet}'-{whole}"));
        if n != 0 {
            out.push_str(&format!(" {n}/{d}"));
        }
    } else if whole != 0 {
        out.push_str(&whole.to_string());
        if n != 0 {
            out.push_str(&format!(" {n}/{d}"));
        }
    } else {
        out.push_str(&format!("{n}/{d}"));
    }
    out.push('"');
}

/// `n/16` in lowest terms.
fn reduce(mut n: i64) -> (i64, i64) {
    let mut d = 16;
    while n != 0 && n % 2 == 0 {
        n /= 2;
        d /= 2;
    }
    (n, d)
}

/// Decimal inches in the canonical (1e-4) form.
pub fn fmt_dec(x: f64) -> String {
    let mut b = [0u8; 40];
    String::from(json::fmt_number(&mut b, x))
}

// ---- scales ----------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// Model inches per paper inch. 0 means NTS (fit to frame).
    pub factor: f64,
}

impl Scale {
    pub fn is_nts(self) -> bool {
        self.factor == 0.0
    }
}

/// Model inches per paper inch accepted for a view: 1:10 enlargement .. 1:10000.
pub const MIN_SCALE_FACTOR: f64 = 0.1;
pub const MAX_SCALE_FACTOR: f64 = 10000.0;

fn finite(f: f64) -> Option<Scale> {
    f.is_finite().then_some(Scale { factor: f })
}

/// A scale `NTS` or one whose factor lies in `MIN_SCALE_FACTOR ..= MAX_SCALE_FACTOR`.
pub fn parse_scale(text: &str) -> Option<Scale> {
    let s = parse_scale_any(text)?;
    if s.factor == 0.0 {
        return Some(s);
    }
    if !(s.factor >= MIN_SCALE_FACTOR && s.factor <= MAX_SCALE_FACTOR) {
        return None;
    }
    Some(s)
}

/// Scale text as written, without the range check (so a message can say "out of range" instead of "unrecognised").
pub fn parse_scale_any(text: &str) -> Option<Scale> {
    let t = text.trim_matches([' ', '\t']);
    if t.eq_ignore_ascii_case("NTS") {
        return Some(Scale { factor: 0.0 });
    }
    if let Some(c) = t.find(':') {
        let a = parse_finite(t[..c].trim_matches(' '))?;
        let b = parse_finite(t[c + 1..].trim_matches(' '))?;
        if a <= 0.0 || b <= 0.0 {
            return None;
        }
        return finite(b / a);
    }
    if let Some(e) = t.find('=') {
        let l = parse_length_str(&t[..e])?;
        let r = parse_length_str(&t[e + 1..])?;
        if l <= 0.0 || r <= 0.0 {
            return None;
        }
        return finite(r / l);
    }
    None
}

/// Scale text for the title block: `1 1/2" = 1'-0"` (architectural), `1:N`, or `NTS`.
pub fn scale_label(factor: f64) -> String {
    if factor == 0.0 {
        return "NTS".to_string();
    }
    let per_foot = 12.0 / factor;
    let sixteenths = per_foot * 16.0;
    if (sixteenths - math::round(sixteenths)).abs() < 1e-6 && (1.0 / 16.0..1e6).contains(&per_foot) {
        let s = num::to_int::<i64>(math::round(sixteenths)).unwrap_or(0);
        let whole = s / 16;
        let (n, d) = reduce(s % 16);
        let mut out = String::new();
        if whole > 0 {
            out.push_str(&whole.to_string());
        }
        if n != 0 {
            if whole > 0 {
                out.push(' ');
            }
            out.push_str(&format!("{n}/{d}"));
        }
        out.push_str("\" = 1'-0\"");
        return out;
    }
    format!("1:{}", fmt_dec(factor))
}

// ---- slopes ----------------------------------------------------------------------------------------

/// `"4:12"` (rise:run) or a number of degrees. Returns radians.
pub fn parse_slope(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => (n.is_finite() && n.abs() <= 360.0).then(|| n * RAD_PER_DEG),
        Value::String(s) => {
            let t = s.trim_matches([' ', '\t']);
            if let Some(c) = t.find(':') {
                let rise = parse_finite(t[..c].trim_matches(' '))?;
                let run = parse_finite(t[c + 1..].trim_matches(' '))?;
                if run == 0.0 || rise.abs() > 1e6 || run.abs() > 1e6 {
                    return None;
                }
                return Some(math::atan2(rise, run));
            }
            let deg = parse_finite(t)?;
            (deg.abs() <= 360.0).then_some(deg * RAD_PER_DEG)
        }
        _ => None,
    }
}
