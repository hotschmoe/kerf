//! Order-preserving JSON value tree. (The parser and writer land here too.)

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

const MAX_DEPTH: u32 = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

/// Parse a UTF-8 JSON document (a leading BOM is skipped). Duplicate keys: the last value wins, in the first key's position.
pub fn parse(src: &[u8]) -> Result<Value, ParseError> {
    let mut p = Parser { src, pos: 0, depth: 0 };
    p.document()
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    depth: u32,
}

type PResult<T> = Result<T, ParseError>;

impl<'a> Parser<'a> {
    fn fail<T>(&self, msg: &str) -> PResult<T> {
        let end = self.pos.min(self.src.len());
        let mut line = 1u32;
        let mut col = 1u32;
        for &c in &self.src[..end] {
            if c == b'\n' {
                line = line.saturating_add(1);
                col = 1;
            } else {
                col = col.saturating_add(1);
            }
        }
        Err(ParseError { line, col, msg: String::from(msg) })
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn document(&mut self) -> PResult<Value> {
        if self.src.starts_with(b"\xEF\xBB\xBF") {
            self.pos = 3;
        }
        self.ws();
        let v = self.value()?;
        self.ws();
        if self.pos != self.src.len() {
            return self.fail("unexpected trailing characters after the JSON value");
        }
        Ok(v)
    }

    fn value(&mut self) -> PResult<Value> {
        let Some(c) = self.peek() else {
            return self.fail("unexpected end of input");
        };
        match c {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.lit(b"true", Value::Bool(true)),
            b'f' => self.lit(b"false", Value::Bool(false)),
            b'n' => self.lit(b"null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => self.fail("unexpected character; expected a JSON value"),
        }
    }

    fn lit(&mut self, word: &[u8], v: Value) -> PResult<Value> {
        if self.src[self.pos..].starts_with(word) {
            self.pos += word.len();
            return Ok(v);
        }
        self.fail("invalid literal")
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> PResult<Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let first_digit = self.pos;
        let digits = self.digits();
        if digits == 0 {
            return self.fail("invalid number");
        }
        if digits > 1 && self.src[first_digit] == b'0' {
            return self.fail("invalid number: leading zeros are not allowed (write 7, not 07)");
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return self.fail("invalid number: digits expected after '.'");
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return self.fail("invalid number exponent");
            }
        }
        let text = core::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
        let Ok(f) = text.parse::<f64>() else {
            return self.fail("invalid number");
        };
        if !(f.abs() <= MAX_ABS_NUMBER) {
            return self.fail("number out of range: a Kerf number must be finite and at most 9e15 in magnitude");
        }
        Ok(Value::Number(f))
    }

    fn hex4(&mut self) -> PResult<u32> {
        let Some(h) = self.src.get(self.pos..self.pos + 4) else {
            return self.fail("truncated \\u escape");
        };
        let mut v = 0u32;
        for &c in h {
            let Some(d) = (c as char).to_digit(16) else {
                return self.fail("invalid \\u escape");
            };
            v = v * 16 + d;
        }
        self.pos += 4;
        Ok(v)
    }

    fn utf8(&self, bytes: Vec<u8>) -> PResult<String> {
        match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(_) => self.fail("string is not valid UTF-8"),
        }
    }

    fn string(&mut self) -> PResult<String> {
        self.pos += 1;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'"' {
                let s = self.utf8(self.src[start..self.pos].to_vec())?;
                self.pos += 1;
                return Ok(s);
            }
            if c == b'\\' {
                break;
            }
            if c < 0x20 {
                return self.fail("control character in string");
            }
            self.pos += 1;
        }
        if self.pos >= self.src.len() {
            return self.fail("unterminated string");
        }
        let mut buf: Vec<u8> = self.src[start..self.pos].to_vec();
        while let Some(c) = self.peek() {
            if c == b'"' {
                let s = self.utf8(buf)?;
                self.pos += 1;
                return Ok(s);
            }
            if c < 0x20 {
                return self.fail("control character in string");
            }
            if c != b'\\' {
                buf.push(c);
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            let Some(e) = self.peek() else { break };
            self.pos += 1;
            match e {
                b'"' => buf.push(b'"'),
                b'\\' => buf.push(b'\\'),
                b'/' => buf.push(b'/'),
                b'b' => buf.push(8),
                b'f' => buf.push(12),
                b'n' => buf.push(b'\n'),
                b'r' => buf.push(b'\r'),
                b't' => buf.push(b'\t'),
                b'u' => {
                    let mut cp = self.hex4()?;
                    if (0xD800..0xDC00).contains(&cp) {
                        if self.src.get(self.pos..self.pos + 2) == Some(b"\\u") {
                            self.pos += 2;
                            let lo = self.hex4()?;
                            if (0xDC00..0xE000).contains(&lo) {
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            } else {
                                return self.fail("invalid surrogate pair");
                            }
                        } else {
                            return self.fail("lone surrogate in \\u escape");
                        }
                    }
                    let Some(ch) = char::from_u32(cp) else {
                        return self.fail("invalid code point");
                    };
                    let mut tmp = [0u8; 4];
                    buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                }
                _ => return self.fail("invalid escape sequence"),
            }
        }
        self.fail("unterminated string")
    }

    fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("nesting too deep");
        }
        Ok(())
    }

    fn array(&mut self) -> PResult<Value> {
        self.enter()?;
        let r = self.array_body();
        self.depth -= 1;
        r
    }

    fn array_body(&mut self) -> PResult<Value> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                None => return self.fail("unterminated array"),
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                Some(_) => return self.fail("expected ',' or ']' in array"),
            }
        }
    }

    fn object(&mut self) -> PResult<Value> {
        self.enter()?;
        let r = self.object_body();
        self.depth -= 1;
        r
    }

    fn object_body(&mut self) -> PResult<Value> {
        self.pos += 1;
        let mut items: Vec<Member> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(items));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return self.fail("expected a string key in object");
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return self.fail("expected ':' after object key");
            }
            self.pos += 1;
            self.ws();
            let value = self.value()?;
            items.push(Member { key, value });
            self.ws();
            match self.peek() {
                None => return self.fail("unterminated object"),
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(dedupe(items)));
                }
                Some(_) => return self.fail("expected ',' or '}' in object"),
            }
        }
    }
}

/// Last duplicate key wins, in the position of the first occurrence. Linear scan for small objects; otherwise a stable sort of
/// member indices by key finds the duplicates in O(n log n).
fn dedupe(mut items: Vec<Member>) -> Vec<Member> {
    if items.len() < 2 {
        return items;
    }
    if items.len() <= 16 {
        let has_dup = items.iter().enumerate().any(|(i, m)| items[..i].iter().any(|p| p.key == m.key));
        if !has_dup {
            return items;
        }
    }
    let mut idx: Vec<usize> = (0..items.len()).collect();
    crate::sort::stable(&mut idx, |&x, &y| {
        items[x].key.as_bytes() < items[y].key.as_bytes() || (items[x].key == items[y].key && x < y)
    });
    let mut drop = alloc::vec![false; items.len()];
    let mut has_dup = false;
    let mut g = 0;
    while g < idx.len() {
        let mut e = g + 1;
        while e < idx.len() && items[idx[e]].key == items[idx[g]].key {
            e += 1;
        }
        if e - g > 1 {
            has_dup = true;
            let last = core::mem::replace(&mut items[idx[e - 1]].value, Value::Null);
            items[idx[g]].value = last;
            for &x in &idx[g + 1..e] {
                drop[x] = true;
            }
        }
        g = e;
    }
    if !has_dup {
        return items;
    }
    let mut i = 0;
    items.retain(|_| {
        i += 1;
        !drop[i - 1]
    });
    items
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
/// printed, saturating at 1e18.
pub fn fmt_fixed(buf: &mut [u8; 40], x: f64, decimals: u8) -> &str {
    if !x.is_finite() {
        return "0";
    }
    let decimals = decimals.min(9);
    let scale: u64 = 10u64.pow(decimals as u32);
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
    let mut ip = ip;
    let mut i = buf.len();
    if frac != 0 {
        let mut digits = decimals as usize;
        while frac % 10 == 0 {
            frac /= 10;
            digits -= 1;
        }
        for _ in 0..digits {
            i -= 1;
            buf[i] = b'0' + (frac % 10) as u8;
            frac /= 10;
        }
        i -= 1;
        buf[i] = b'.';
    }
    if ip == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while ip > 0 {
            i -= 1;
            buf[i] = b'0' + (ip % 10) as u8;
            ip /= 10;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
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
