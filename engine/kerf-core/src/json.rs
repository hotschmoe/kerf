//! Order-preserving JSON value tree, the parser (total, nesting at most 200, every string valid UTF-8) and the writers.

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

/// Deepest array/object nesting the parser accepts.
pub const MAX_DEPTH: u32 = 200;

/// Objects up to this many members are checked for duplicate keys pairwise; larger ones are sorted.
const SMALL_OBJECT: usize = 16;

/// Where a document is not JSON: 1-based line, 1-based column in bytes, and what is wrong.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

struct Fail {
    pos: usize,
    msg: &'static str,
}

type Parsed<T> = Result<T, Fail>;

fn fail<T>(pos: usize, msg: &'static str) -> Parsed<T> {
    Err(Fail { pos, msg })
}

enum NumberError {
    Invalid,
    OutOfRange,
}

/// The value of a lexeme that matched the JSON number grammar, if it is finite and at most `MAX_ABS_NUMBER`.
/// Correctly rounded decimal-to-binary conversion is core's `f64::from_str`.
fn number_value(lexeme: &[u8]) -> Result<f64, NumberError> {
    let f: f64 = core::str::from_utf8(lexeme).ok().and_then(|t| t.parse().ok()).ok_or(NumberError::Invalid)?;
    if f.abs() <= MAX_ABS_NUMBER { Ok(f) } else { Err(NumberError::OutOfRange) }
}

/// A recursive-descent parser over bytes (strict RFC 8259 grammar). Total: no input panics, overflows or loops forever.
struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_while(&mut self, pred: impl Fn(u8) -> bool) {
        self.pos += self.src[self.pos..].iter().take_while(|&&c| pred(c)).count();
    }

    fn skip_ws(&mut self) {
        self.skip_while(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r'));
    }

    fn value(&mut self, depth: u32) -> Parsed<Value> {
        match self.peek() {
            None => fail(self.pos, "unexpected end of input"),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number().map(Value::Number),
            Some(_) => fail(self.pos, "unexpected character; expected a JSON value"),
        }
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Parsed<Value> {
        if !self.src[self.pos..].starts_with(word) {
            return fail(self.pos, "invalid literal");
        }
        self.pos += word.len();
        Ok(value)
    }

    fn number(&mut self) -> Parsed<f64> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let int_start = self.pos;
        self.skip_while(|c| c.is_ascii_digit());
        if self.pos == int_start {
            return fail(self.pos, "invalid number");
        }
        if self.pos - int_start > 1 && self.src[int_start] == b'0' {
            return fail(self.pos, "invalid number: leading zeros are not allowed (write 7, not 07)");
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let frac_start = self.pos;
            self.skip_while(|c| c.is_ascii_digit());
            if self.pos == frac_start {
                return fail(self.pos, "invalid number: digits expected after '.'");
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            self.skip_while(|c| c.is_ascii_digit());
            if self.pos == exp_start {
                return fail(self.pos, "invalid number exponent");
            }
        }
        number_value(&self.src[start..self.pos]).or_else(|e| match e {
            NumberError::Invalid => fail(self.pos, "invalid number"),
            NumberError::OutOfRange => {
                fail(self.pos, "number out of range: a Kerf number must be finite and at most 9e15 in magnitude")
            }
        })
    }

    /// The four hex digits of a `\u` escape at `at`.
    fn hex4(&self, at: usize) -> Parsed<u32> {
        let Some(digits) = self.src.get(at..at + 4) else { return fail(at, "truncated \\u escape") };
        digits
            .iter()
            .try_fold(0, |v, &c| char::from(c).to_digit(16).map(|d| v * 16 + d))
            .ok_or(Fail { pos: at, msg: "invalid \\u escape" })
    }

    /// A `\uXXXX` escape (surrogate pairs combined) whose hex digits start at `at`; returns where the string goes on.
    fn unicode_escape(&self, at: usize, buf: &mut Vec<u8>) -> Parsed<usize> {
        let mut cp = self.hex4(at)?;
        let mut next = at + 4;
        if (0xd800..0xdc00).contains(&cp) {
            if !self.src[next..].starts_with(b"\\u") {
                return fail(next, "lone surrogate in \\u escape");
            }
            let low = self.hex4(next + 2)?;
            next += 6;
            if !(0xdc00..0xe000).contains(&low) {
                return fail(next, "invalid surrogate pair");
            }
            cp = 0x10000 + ((cp - 0xd800) << 10) + (low - 0xdc00);
        }
        let Some(c) = char::from_u32(cp) else { return fail(next, "invalid code point") };
        buf.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
        Ok(next)
    }

    /// An escape sequence whose first byte after the backslash is at `at`; returns where the string goes on.
    fn escape(&self, at: usize, buf: &mut Vec<u8>) -> Parsed<usize> {
        let Some(&e) = self.src.get(at) else { return fail(at, "unterminated string") };
        let byte = match e {
            b'"' | b'\\' | b'/' => e,
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'u' => return self.unicode_escape(at + 1, buf),
            _ => return fail(at + 1, "invalid escape sequence"),
        };
        buf.push(byte);
        Ok(at + 1)
    }

    /// A string whose opening quote is at the cursor.
    fn string(&mut self) -> Parsed<String> {
        let mut at = self.pos + 1;
        let mut buf = Vec::new();
        loop {
            let run = self.src[at..].iter().position(|&c| c == b'"' || c == b'\\' || c < 0x20);
            let Some(run) = run else { return fail(self.src.len(), "unterminated string") };
            buf.extend_from_slice(&self.src[at..at + run]);
            at += run;
            match self.src[at] {
                b'"' => {
                    self.pos = at + 1;
                    return String::from_utf8(buf).or_else(|_| fail(at, "string is not valid UTF-8"));
                }
                b'\\' => at = self.escape(at + 1, &mut buf)?,
                _ => return fail(at, "control character in string"),
            }
        }
    }

    fn array(&mut self, depth: u32) -> Parsed<Value> {
        if depth >= MAX_DEPTH {
            return fail(self.pos, "nesting too deep");
        }
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.peek() {
                None => return fail(self.pos, "unterminated array"),
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                Some(_) => return fail(self.pos, "expected ',' or ']' in array"),
            }
        }
    }

    fn object(&mut self, depth: u32) -> Parsed<Value> {
        if depth >= MAX_DEPTH {
            return fail(self.pos, "nesting too deep");
        }
        self.pos += 1;
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return fail(self.pos, "expected a string key in object");
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return fail(self.pos, "expected ':' after object key");
            }
            self.pos += 1;
            self.skip_ws();
            members.push(Member { key, value: self.value(depth + 1)? });
            self.skip_ws();
            match self.peek() {
                None => return fail(self.pos, "unterminated object"),
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(dedupe(members)));
                }
                Some(_) => return fail(self.pos, "expected ',' or '}' in object"),
            }
        }
    }
}

/// Duplicate keys: the last value wins, in the position of the first occurrence.
fn dedupe(mut members: Vec<Member>) -> Vec<Member> {
    let n = members.len();
    let small_and_unique = n <= SMALL_OBJECT && (1..n).all(|i| members[..i].iter().all(|m| m.key != members[i].key));
    if n < 2 || small_and_unique {
        return members;
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_unstable_by(|&x, &y| members[x].key.cmp(&members[y].key).then(x.cmp(&y)));
    let run_lens: Vec<usize> = order.chunk_by(|&x, &y| members[x].key == members[y].key).map(<[usize]>::len).collect();
    let mut keep = alloc::vec![true; n];
    let mut start = 0;
    for len in run_lens {
        let run = &order[start..start + len];
        start += len;
        if let [first, rest @ .., last] = run {
            members[*first].value = core::mem::replace(&mut members[*last].value, Value::Null);
            rest.iter().chain([last]).for_each(|&i| keep[i] = false);
        }
    }
    let mut keep = keep.into_iter();
    members.retain(|_| keep.next() == Some(true));
    members
}

/// 1-based line and byte column of offset `pos`.
fn location(src: &[u8], pos: usize) -> (u32, u32) {
    let before = &src[..pos.min(src.len())];
    let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
    let col = before.len() - before.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1) + 1;
    (u32::try_from(line).unwrap_or(u32::MAX), u32::try_from(col).unwrap_or(u32::MAX))
}

/// Parses a JSON document: strict RFC 8259 grammar, a leading UTF-8 BOM skipped, nesting at most `MAX_DEPTH`, numbers
/// at most `MAX_ABS_NUMBER` in magnitude, duplicate keys resolved last-wins in the first key's position. Every string and
/// key of the result is valid UTF-8.
pub fn parse(src: &[u8]) -> Result<Value, ParseError> {
    let mut p = Parser { src, pos: if src.starts_with(&[0xEF, 0xBB, 0xBF]) { 3 } else { 0 } };
    p.skip_ws();
    let parsed = p.value(0).and_then(|v| {
        p.skip_ws();
        if p.pos == src.len() { Ok(v) } else { fail(p.pos, "unexpected trailing characters after the JSON value") }
    });
    parsed.map_err(|f| {
        let (line, col) = location(src, f.pos);
        ParseError { line, col, msg: String::from(f.msg) }
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
