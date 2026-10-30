//! Order-preserving JSON value tree, the parser (total, nesting at most 200, every string valid UTF-8) and the writers.

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::math;
use crate::num::to_int_clamped;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub key: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<Member>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.iter().find(|m| m.key == key).map(|m| &m.value),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Object(m) => m.iter_mut().find(|m| m.key == key).map(|m| &mut m.value),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s) } else { None }
    }
    pub fn num(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }
    pub fn arr(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    /// The type name used in error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }
}

/// Largest magnitude the parser accepts (2^53 is the largest range where doubles are exact integers).
pub const MAX_ABS_NUMBER: f64 = 9.0e15;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

/// Deepest array/object nesting the parser accepts (json.zig).
pub const MAX_DEPTH: u32 = 200;

struct Fail {
    pos: usize,
    msg: &'static str,
}

#[inline(always)]
fn owned(msg: &str) -> String {
    String::from(msg)
}

/// The bytes of a member's key .
#[inline(always)]
fn key_bytes(m: &Member) -> &[u8] {
    m.key.as_bytes()
}

/// The value of a lexeme that matched the JSON number grammar, if it is within json.zig's
/// `max_abs_number` (9e15). Correctly rounded decimal-to-binary
/// conversion is core's `f64::from_str` (as Zig's `std.fmt.parseFloat`).
fn number_value(lexeme: &[u8]) -> Result<f64, bool> {
    let Ok(text) = core::str::from_utf8(lexeme) else {
        return Err(false);
    };
    let Ok(f) = text.parse::<f64>() else {
        return Err(false);
    };
    if f.abs() <= 9.0e15 { Ok(f) } else { Err(true) }
}

// ---- parser (json.zig `Parser`, same acceptance, values and error positions) ---------------------

#[inline(always)]
fn is_ws(c: u8) -> bool {
    c == 32 || c == 9 || c == 10 || c == 13
}

#[inline(always)]
fn ws(src: &[u8], pos: usize) -> usize {
    let mut p = pos;
    while p < src.len() && is_ws(src[p]) {
        p += 1;
    }
    p
}

#[inline(always)]
fn is_digit(c: u8) -> bool {
    48 <= c && c <= 57
}

fn starts_with(src: &[u8], pos: usize, word: &[u8]) -> bool {
    if src.len() - pos < word.len() {
        return false;
    }
    let mut k: usize = 0;
    while k < word.len() {
        if src[pos + k] != word[k] {
            return false;
        }
        k += 1;
    }
    true
}

fn slice_to_vec(src: &[u8], a: usize, b: usize) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(&src[a..b]);
    v
}

fn value(src: &[u8], pos: usize, depth: u32) -> Result<(Value, usize), Fail> {
    if pos >= src.len() {
        return Err(Fail {
            pos,
            msg: "unexpected end of input",
        });
    }
    let c = src[pos];
    if c == 123 {
        object(src, pos, depth)
    } else if c == 91 {
        array(src, pos, depth)
    } else if c == 34 {
        match parse_string(src, pos) {
            Ok((s, p)) => Ok((Value::String(s), p)),
            Err(f) => Err(f),
        }
    } else if c == 116 {
        if starts_with(src, pos, &[116u8, 114, 117, 101]) {
            Ok((Value::Bool(true), pos + 4))
        } else {
            Err(Fail {
                pos,
                msg: "invalid literal",
            })
        }
    } else if c == 102 {
        if starts_with(src, pos, &[102u8, 97, 108, 115, 101]) {
            Ok((Value::Bool(false), pos + 5))
        } else {
            Err(Fail {
                pos,
                msg: "invalid literal",
            })
        }
    } else if c == 110 {
        if starts_with(src, pos, &[110u8, 117, 108, 108]) {
            Ok((Value::Null, pos + 4))
        } else {
            Err(Fail {
                pos,
                msg: "invalid literal",
            })
        }
    } else if c == 45 || is_digit(c) {
        match number(src, pos) {
            Ok((f, p)) => Ok((Value::Number(f), p)),
            Err(f) => Err(f),
        }
    } else {
        Err(Fail {
            pos,
            msg: "unexpected character; expected a JSON value",
        })
    }
}

#[inline(always)]
fn digits(src: &[u8], pos: usize) -> usize {
    let mut p = pos;
    while p < src.len() && is_digit(src[p]) {
        p += 1;
    }
    p
}

fn number(src: &[u8], pos: usize) -> Result<(f64, usize), Fail> {
    let start = pos;
    let mut p = pos;
    if src[p] == 45 {
        p += 1;
    }
    let first = p;
    p = digits(src, p);
    if p == first {
        return Err(Fail {
            pos: p,
            msg: "invalid number",
        });
    }
    if p - first > 1 && src[first] == 48 {
        return Err(Fail {
            pos: p,
            msg: "invalid number: leading zeros are not allowed (write 7, not 07)",
        });
    }
    if p < src.len() && src[p] == 46 {
        p += 1;
        let f = p;
        p = digits(src, p);
        if p == f {
            return Err(Fail {
                pos: p,
                msg: "invalid number: digits expected after '.'",
            });
        }
    }
    if p < src.len() && (src[p] == 101 || src[p] == 69) {
        p += 1;
        if p < src.len() && (src[p] == 43 || src[p] == 45) {
            p += 1;
        }
        let e = p;
        p = digits(src, p);
        if p == e {
            return Err(Fail {
                pos: p,
                msg: "invalid number exponent",
            });
        }
    }
    match number_value(&src[start..p]) {
        Ok(f) => Ok((f, p)),
        Err(false) => Err(Fail {
            pos: p,
            msg: "invalid number",
        }),
        Err(true) => Err(Fail {
            pos: p,
            msg: "number out of range: a Kerf number must be finite and at most 9e15 in magnitude",
        }),
    }
}

fn hex4(src: &[u8], pos: usize) -> Result<u32, Fail> {
    if src.len() - pos < 4 {
        return Err(Fail {
            pos,
            msg: "truncated \\u escape",
        });
    }
    let mut v: u32 = 0;
    let mut k: usize = 0;
    while k < 4 {
        let c = src[pos + k];
        let d: u32 = if 48 <= c && c <= 57 {
            (c - 48) as u32
        } else if 97 <= c && c <= 102 {
            (c - 97 + 10) as u32
        } else if 65 <= c && c <= 70 {
            (c - 65 + 10) as u32
        } else {
            return Err(Fail {
                pos,
                msg: "invalid \\u escape",
            });
        };
        v = v * 16 + d;
        k += 1;
    }
    Ok(v)
}

/// UTF-8 bytes of a scalar value (`std.unicode.utf8Encode`).
fn push_utf8(buf: &mut Vec<u8>, cp: u32) {
    if cp < 0x80 {
        buf.push(cp as u8);
    } else if cp < 0x800 {
        buf.push((0xc0 | (cp >> 6)) as u8);
        buf.push((0x80 | (cp & 0x3f)) as u8);
    } else if cp < 0x10000 {
        buf.push((0xe0 | (cp >> 12)) as u8);
        buf.push((0x80 | ((cp >> 6) & 0x3f)) as u8);
        buf.push((0x80 | (cp & 0x3f)) as u8);
    } else {
        buf.push((0xf0 | (cp >> 18)) as u8);
        buf.push((0x80 | ((cp >> 12) & 0x3f)) as u8);
        buf.push((0x80 | ((cp >> 6) & 0x3f)) as u8);
        buf.push((0x80 | (cp & 0x3f)) as u8);
    }
}

fn parse_string(src: &[u8], pos: usize) -> Result<(String, usize), Fail> {
    let start = pos + 1;
    let mut p = start;
    while p < src.len() {
        let c = src[p];
        if c == 34 {
            return match String::from_utf8(slice_to_vec(src, start, p)) {
                Ok(s) => Ok((s, p + 1)),
                Err(_) => Err(Fail { pos: p, msg: "string is not valid UTF-8" }),
            };
        }
        if c == 92 {
            break;
        }
        if c < 0x20 {
            return Err(Fail {
                pos: p,
                msg: "control character in string",
            });
        }
        p += 1;
    }
    if p >= src.len() {
        return Err(Fail {
            pos: p,
            msg: "unterminated string",
        });
    }
    let mut buf = slice_to_vec(src, start, p);
    while p < src.len() {
        let c = src[p];
        if c == 34 {
            return match String::from_utf8(buf) {
                Ok(s) => Ok((s, p + 1)),
                Err(_) => Err(Fail { pos: p, msg: "string is not valid UTF-8" }),
            };
        }
        if c < 0x20 {
            return Err(Fail {
                pos: p,
                msg: "control character in string",
            });
        }
        if c != 92 {
            buf.push(c);
            p += 1;
            continue;
        }
        p += 1;
        if p >= src.len() {
            break;
        }
        let e = src[p];
        p += 1;
        if e == 34 {
            buf.push(34);
        } else if e == 92 {
            buf.push(92);
        } else if e == 47 {
            buf.push(47);
        } else if e == 98 {
            buf.push(8);
        } else if e == 102 {
            buf.push(12);
        } else if e == 110 {
            buf.push(10);
        } else if e == 114 {
            buf.push(13);
        } else if e == 116 {
            buf.push(9);
        } else if e == 117 {
            let mut cp = match hex4(src, p) {
                Ok(v) => v,
                Err(f) => return Err(f),
            };
            p += 4;
            if 0xd800 <= cp && cp < 0xdc00 {
                if src.len() - p >= 2 && src[p] == 92 && src[p + 1] == 117 {
                    p += 2;
                    let lo = match hex4(src, p) {
                        Ok(v) => v,
                        Err(f) => return Err(f),
                    };
                    p += 4;
                    if 0xdc00 <= lo && lo < 0xe000 {
                        let t = cp - 0xd800;
                        cp = 0x10000 + (t << 10) + (lo - 0xdc00);
                    } else {
                        return Err(Fail {
                            pos: p,
                            msg: "invalid surrogate pair",
                        });
                    }
                } else {
                    return Err(Fail {
                        pos: p,
                        msg: "lone surrogate in \\u escape",
                    });
                }
            }
            if 0xd800 <= cp && cp < 0xe000 {
                return Err(Fail {
                    pos: p,
                    msg: "invalid code point",
                });
            }
            push_utf8(&mut buf, cp);
        } else {
            return Err(Fail {
                pos: p,
                msg: "invalid escape sequence",
            });
        }
    }
    Err(Fail {
        pos: p,
        msg: "unterminated string",
    })
}

fn array(src: &[u8], pos: usize, depth: u32) -> Result<(Value, usize), Fail> {
    if depth >= MAX_DEPTH {
        return Err(Fail {
            pos,
            msg: "nesting too deep",
        });
    }
    let mut items: Vec<Value> = Vec::new();
    let mut p = ws(src, pos + 1);
    if p < src.len() && src[p] == 93 {
        return Ok((Value::Array(items), p + 1));
    }
    loop {
        p = ws(src, p);
        match value(src, p, depth + 1) {
            Ok((v, q)) => {
                items.push(v);
                p = ws(src, q);
            }
            Err(f) => return Err(f),
        }
        if p >= src.len() {
            return Err(Fail {
                pos: p,
                msg: "unterminated array",
            });
        }
        if src[p] == 44 {
            p += 1;
        } else if src[p] == 93 {
            return Ok((Value::Array(items), p + 1));
        } else {
            return Err(Fail {
                pos: p,
                msg: "expected ',' or ']' in array",
            });
        }
    }
}

fn object(src: &[u8], pos: usize, depth: u32) -> Result<(Value, usize), Fail> {
    if depth >= MAX_DEPTH {
        return Err(Fail {
            pos,
            msg: "nesting too deep",
        });
    }
    let mut items: Vec<Member> = Vec::new();
    let mut p = ws(src, pos + 1);
    if p < src.len() && src[p] == 125 {
        return Ok((Value::Object(items), p + 1));
    }
    loop {
        p = ws(src, p);
        if p >= src.len() || src[p] != 34 {
            return Err(Fail {
                pos: p,
                msg: "expected a string key in object",
            });
        }
        let (k, q) = match parse_string(src, p) {
            Ok(x) => x,
            Err(f) => return Err(f),
        };
        p = ws(src, q);
        if p >= src.len() || src[p] != 58 {
            return Err(Fail {
                pos: p,
                msg: "expected ':' after object key",
            });
        }
        p = ws(src, p + 1);
        match value(src, p, depth + 1) {
            Ok((v, q)) => {
                items.push(Member { key: k, value: v });
                p = ws(src, q);
            }
            Err(f) => return Err(f),
        }
        if p >= src.len() {
            return Err(Fail {
                pos: p,
                msg: "unterminated object",
            });
        }
        if src[p] == 44 {
            p += 1;
        } else if src[p] == 125 {
            let ms = dedupe(items);
            return Ok((Value::Object(ms), p + 1));
        } else {
            return Err(Fail {
                pos: p,
                msg: "expected ',' or '}' in object",
            });
        }
    }
}

// ---- duplicate keys: the last value wins, in the position of the first occurrence ---------------

fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut k: usize = 0;
    while k < a.len() {
        if a[k] != b[k] {
            return false;
        }
        k += 1;
    }
    true
}

/// `std.mem.order(u8, a, b) == .lt`
fn bytes_lt(a: &[u8], b: &[u8]) -> bool {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut k: usize = 0;
    while k < n {
        if a[k] != b[k] {
            return a[k] < b[k];
        }
        k += 1;
    }
    a.len() < b.len()
}

/// json.zig `keyLess`: by key, then by position (so the sort order is total).
fn key_less(items: &Vec<Member>, x: usize, y: usize) -> bool {
    if x >= items.len() || y >= items.len() {
        return false;
    }
    let (a, b) = (key_bytes(&items[x]), key_bytes(&items[y]));
    if bytes_lt(a, b) {
        true
    } else if bytes_lt(b, a) {
        false
    } else {
        x < y
    }
}

fn dedupe(items: Vec<Member>) -> Vec<Member> {
    let mut items = items;
    let n = items.len();
    if n < 2 {
        return items;
    }
    if n <= 16 {
        let mut dup = false;
        let mut i: usize = 1;
        while i < n && !dup {
            let mut j: usize = 0;
            while j < i && !dup {
                if bytes_eq(key_bytes(&items[i]), key_bytes(&items[j])) {
                    dup = true;
                }
                j += 1;
            }
            i += 1;
        }
        if !dup {
            return items;
        }
    }
    let mut idx: Vec<usize> = Vec::with_capacity(n);
    let mut i: usize = 0;
    while i < n {
        idx.push(i);
        i += 1;
    }
    {
        let its = &items;
        crate::sort::stable(idx.as_mut_slice(), |x: &usize, y: &usize| {
            key_less(its, *x, *y)
        });
    }
    let mut drop: Vec<bool> = Vec::with_capacity(n);
    let mut i: usize = 0;
    while i < n {
        drop.push(false);
        i += 1;
    }
    let mut has_dup = false;
    let mut g: usize = 0;
    while g < n {
        let mut e = g + 1;
        while e < n && bytes_eq(key_bytes(&items[idx[e]]), key_bytes(&items[idx[g]])) {
            e += 1;
        }
        if e - g > 1 {
            has_dup = true;
            let j = idx[e - 1];
            let i = idx[g];
            let mut last = Value::Null;
            core::mem::swap(&mut items[j].value, &mut last);
            items[i].value = last;
            let mut k = g + 1;
            while k < e {
                drop[idx[k]] = true;
                k += 1;
            }
        }
        g = e;
    }
    if !has_dup {
        return items;
    }
    let mut out: Vec<Member> = Vec::with_capacity(n);
    let mut rev: Vec<Member> = Vec::with_capacity(n);
    while items.len() > 0 {
        match items.pop() {
            Some(m) => rev.push(m),
            None => {}
        }
    }
    let mut i: usize = 0;
    while rev.len() > 0 {
        match rev.pop() {
            Some(m) => {
                if !drop[i] {
                    out.push(m);
                }
            }
            None => {}
        }
        i += 1;
    }
    out
}

// ---- entry point --------------------------------------------------------------------------------

fn location(src: &[u8], pos: usize) -> (u32, u32) {
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    let end = if pos < src.len() { pos } else { src.len() };
    let mut k: usize = 0;
    while k < end {
        if src[k] == 10 {
            if line < u32::MAX {
                line += 1;
            }
            col = 1;
        } else if col < u32::MAX {
            col += 1;
        }
        k += 1;
    }
    (line, col)
}

/// Parses a JSON document exactly as json.zig's `parse` does (strict RFC 8259 grammar, a leading
/// UTF-8 BOM skipped, nesting at most 200, numbers at most 9e15 in magnitude, duplicate keys: last
/// value wins in the first key's position). Total: no panic, no overflow, terminates on every
/// input. On success every string and key is valid UTF-8 and nesting is within the limit.
pub fn parse(src: &[u8]) -> Result<Value, ParseError> {
    let mut pos: usize = 0;
    if starts_with(src, 0, &[0xEFu8, 0xBB, 0xBF]) {
        pos = 3;
    }
    pos = ws(src, pos);
    let f = match value(src, pos, 0) {
        Ok((v, p)) => {
            let q = ws(src, p);
            if q == src.len() {
                return Ok(v);
            }
            Fail {
                pos: q,
                msg: "unexpected trailing characters after the JSON value",
            }
        }
        Err(f) => f,
    };
    let (line, col) = location(src, f.pos);
    Err(ParseError {
        line,
        col,
        msg: owned(f.msg),
    })
}

// ---- numbers -------------------------------------------------------------------------------------

/// Kerf number format: round to 1e-4, shortest decimal, no exponent, `-0` -> `0`.
pub fn fmt_number(buf: &mut [u8; 40], x: f64) -> &str {
    fmt_fixed(buf, x, 4)
}

pub fn number_string(x: f64) -> String {
    let mut b = [0u8; 40];
    String::from(fmt_number(&mut b, x))
}

pub fn push_number(out: &mut Vec<u8>, x: f64) {
    let mut b = [0u8; 40];
    out.extend_from_slice(fmt_number(&mut b, x).as_bytes());
}

/// The one number formatter of the engine: `x` rounded to `decimals` places (at most 9), printed as the shortest decimal
/// with no exponent, `-0` -> `0`, NaN/inf -> `0`. Integer arithmetic only, so it is identical on every target, never allocates and
/// can not overflow its buffer: beyond 9e15 / 10^decimals the fraction no longer fits a double and the rounded integer is
/// printed, saturating at 1e18. Monomorphized per decimal count (Zig's `comptime decimals`), so `10^decimals` divides by a
/// constant; the digits are ASCII.
pub fn fmt_fixed(buf: &mut [u8; 40], x: f64, decimals: u8) -> &str {
    match decimals {
        0 => fmt_fixed_n::<0>(buf, x),
        1 => fmt_fixed_n::<1>(buf, x),
        2 => fmt_fixed_n::<2>(buf, x),
        3 => fmt_fixed_n::<3>(buf, x),
        4 => fmt_fixed_n::<4>(buf, x),
        5 => fmt_fixed_n::<5>(buf, x),
        6 => fmt_fixed_n::<6>(buf, x),
        7 => fmt_fixed_n::<7>(buf, x),
        8 => fmt_fixed_n::<8>(buf, x),
        _ => fmt_fixed_n::<9>(buf, x),
    }
}

#[inline(always)]
fn fmt_fixed_n<const D: u32>(buf: &mut [u8; 40], x: f64) -> &str {
    if !x.is_finite() {
        return "0";
    }
    let scale: u64 = 10u64.pow(D);
    let scale_f = scale as f64;
    let neg = x < 0.0;
    let ip;
    let mut frac = 0u64;
    if x.abs() >= 9.0e15 / scale_f {
        ip = to_int_clamped::<u64>(math::round(x.abs()).min(1.0e18), 0, u64::MAX);
    } else {
        let n = to_int_clamped::<u64>(math::round(x.abs() * scale_f), 0, u64::MAX);
        if n == 0 {
            return "0";
        }
        ip = n / scale;
        frac = n % scale;
    }
    let i = write_fixed(buf, neg, ip, frac, D as usize);
    ascii_str(buf, i)
}

/// Writes `ip`, then `.` and the `digits`-digit fraction `frac` without trailing zeros (none if `frac` is 0), then a
/// leading `-` if `neg`, right-aligned at the end of `buf`; returns where the text starts. Every byte written is ASCII.
#[inline(always)]
fn write_fixed(buf: &mut [u8], neg: bool, ip: u64, frac: u64, digits: usize) -> usize {
    let mut ip = ip;
    let mut frac = frac;
    let mut digits = digits;
    let mut i: usize = 40;
    if frac != 0 {
        while digits > 0 && frac % 10 == 0 {
            frac /= 10;
            digits -= 1;
        }
        while digits > 0 {
            i -= 1;
            buf[i] = b'0' + (frac % 10) as u8;
            frac /= 10;
            digits -= 1;
        }
        i -= 1;
        buf[i] = b'.';
    }
    if ip == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        i -= 1;
        buf[i] = b'0' + (ip % 10) as u8;
        ip /= 10;
        while ip > 0 && i > 1 {
            i -= 1;
            buf[i] = b'0' + (ip % 10) as u8;
            ip /= 10;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    i
}

/// The digits `write_fixed` wrote are ASCII.
fn ascii_str(buf: &[u8], i: usize) -> &str {
    core::str::from_utf8(&buf[i..]).unwrap_or("0")
}

/// Round to 1e-4 (the canonical precision).
pub fn round4(x: f64) -> f64 {
    let r = math::round(x * 10000.0) / 10000.0;
    if r == 0.0 { 0.0 } else { r }
}

// ---- writer --------------------------------------------------------------------------------------

pub fn write_string(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for &c in s.as_bytes() {
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0..=8 | 11 | 12 | 14..=31 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.extend_from_slice(b"\\u00");
                out.push(HEX[(c >> 4) as usize]);
                out.push(HEX[(c & 15) as usize]);
            }
            _ => out.push(c),
        }
    }
    out.push(b'"');
}

/// What a key-order hook sees: the path of keys from the root to the object (array elements show as `[]`) and the object.
pub struct KeyCtx<'a, 'p> {
    pub path: &'a [&'p str],
    pub obj: &'a [Member],
}

/// Order-fixing hook for the canonical writer: the schema key order for the object described by the context.
pub type KeyOrderFn = fn(&KeyCtx) -> Vec<String>;

pub struct Pretty<'o> {
    pub out: &'o mut Vec<u8>,
    pub order: Option<KeyOrderFn>,
    /// Arrays whose elements are all numbers print inline when true (canonical doc form).
    pub inline_numbers: bool,
}

impl<'o> Pretty<'o> {
    pub fn new(out: &'o mut Vec<u8>) -> Self {
        Pretty { out, order: None, inline_numbers: true }
    }

    pub fn write(&mut self, v: &Value, indent: usize) {
        let mut path = Vec::new();
        self.node(v, indent, &mut path);
    }

    fn pad(&mut self, n: usize) {
        for _ in 0..n {
            self.out.extend_from_slice(b"  ");
        }
    }

    fn node<'v>(&mut self, v: &'v Value, indent: usize, path: &mut Vec<&'v str>) {
        match v {
            Value::Null => self.out.extend_from_slice(b"null"),
            Value::Bool(b) => self.out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Value::Number(n) => push_number(self.out, *n),
            Value::String(s) => write_string(self.out, s),
            Value::Array(items) => {
                if items.is_empty() {
                    self.out.extend_from_slice(b"[]");
                    return;
                }
                if self.inline_numbers && items.iter().all(|it| matches!(it, Value::Number(_))) {
                    self.out.push(b'[');
                    for (i, it) in items.iter().enumerate() {
                        if i > 0 {
                            self.out.extend_from_slice(b", ");
                        }
                        if let Value::Number(n) = it {
                            push_number(self.out, *n);
                        }
                    }
                    self.out.push(b']');
                    return;
                }
                self.out.extend_from_slice(b"[\n");
                path.push("[]");
                for (i, it) in items.iter().enumerate() {
                    self.pad(indent + 1);
                    self.node(it, indent + 1, path);
                    if i + 1 < items.len() {
                        self.out.push(b',');
                    }
                    self.out.push(b'\n');
                }
                path.pop();
                self.pad(indent);
                self.out.push(b']');
            }
            Value::Object(members) => {
                if members.is_empty() {
                    self.out.extend_from_slice(b"{}");
                    return;
                }
                let ordered = self.order_members(members, path);
                self.out.extend_from_slice(b"{\n");
                for (i, m) in ordered.iter().enumerate() {
                    self.pad(indent + 1);
                    write_string(self.out, &m.key);
                    self.out.extend_from_slice(b": ");
                    path.push(&m.key);
                    self.node(&m.value, indent + 1, path);
                    path.pop();
                    if i + 1 < ordered.len() {
                        self.out.push(b',');
                    }
                    self.out.push(b'\n');
                }
                self.pad(indent);
                self.out.push(b'}');
            }
        }
    }

    fn order_members<'v>(&self, members: &'v [Member], path: &[&str]) -> Vec<&'v Member> {
        let Some(f) = self.order else {
            return members.iter().collect();
        };
        let schema = f(&KeyCtx { path, obj: members });
        let mut out: Vec<&Member> = Vec::with_capacity(members.len());
        let mut used = alloc::vec![false; members.len()];
        for k in &schema {
            if let Some(i) = (0..members.len()).find(|&i| !used[i] && members[i].key == *k) {
                out.push(&members[i]);
                used[i] = true;
            }
        }
        let start = out.len();
        for (i, m) in members.iter().enumerate() {
            if !used[i] {
                out.push(m);
            }
        }
        crate::sort::stable(&mut out[start..], |x, y| x.key.as_bytes() < y.key.as_bytes());
        out
    }
}

/// Compact one-line JSON (no whitespace), using the Kerf number format.
pub fn write_compact(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => push_number(out, *n),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push(b'[');
            for (i, it) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_compact(out, it);
            }
            out.push(b']');
        }
        Value::Object(ms) => {
            out.push(b'{');
            for (i, m) in ms.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(out, &m.key);
                out.push(b':');
                write_compact(out, &m.value);
            }
            out.push(b'}');
        }
    }
}

// ---- builders --------------------------------------------------------------------------------------

pub fn obj<K: Into<String>>(members: impl IntoIterator<Item = (K, Value)>) -> Value {
    Value::Object(members.into_iter().map(|(k, value)| Member { key: k.into(), value }).collect())
}

pub fn arr_of(items: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(items.into_iter().collect())
}

pub fn num_arr(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|&x| Value::Number(x)).collect())
}

pub fn string(s: &str) -> Value {
    Value::String(String::from(s))
}

/// Structural equality; object member order does not matter.
pub fn eql(x: &Value, y: &Value) -> bool {
    match (x, y) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(p, q)| eql(p, q)),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|m| y.get(&m.key).is_some_and(|o| eql(&m.value, o)))
        }
        _ => false,
    }
}

/// RFC 7396 merge patch.
pub fn merge_patch(target: &Value, patch: &Value) -> Value {
    let Value::Object(pms) = patch else {
        return patch.clone();
    };
    let mut members: Vec<Member> = match target {
        Value::Object(ms) => ms.clone(),
        _ => Vec::new(),
    };
    for pm in pms {
        let idx = members.iter().position(|m| m.key == pm.key);
        if pm.value.is_null() {
            if let Some(i) = idx {
                members.remove(i);
            }
            continue;
        }
        match idx {
            Some(i) => members[i].value = merge_patch(&members[i].value, &pm.value),
            None => members.push(Member { key: pm.key.clone(), value: merge_patch(&Value::Null, &pm.value) }),
        }
    }
    Value::Object(members)
}
