//! Order-preserving JSON value tree, the parser (proven with Verus, engines/rust/verus/VERUS.md: total, terminating,
//! nesting at most 200, every string valid UTF-8 by proof rather than a second check), and the writers (not verified).

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::math;
use crate::num::to_int_clamped;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;
#[cfg(verus_keep_ghost)]
use vstd::utf8::*;
#[cfg(verus_keep_ghost)]
use crate::sort::{is_perm, tracks};

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verifier::external_derive)]
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub key: String,
    pub value: Value,
}

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verifier::external_derive)]
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

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verifier::external_derive)]
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

/// Deepest array/object nesting the parser accepts (json.zig).
#[cfg_attr(verus_keep_ghost, verus_verify)]
pub const MAX_DEPTH: u32 = 200;




#[cfg_attr(verus_keep_ghost, verus_verify)]
struct Fail {
    pos: usize,
    msg: &'static str,
}

#[cfg(verus_keep_ghost)]
verus! {

/// Every string (and object key) in `v` is valid UTF-8, and arrays and objects nest at most `d`
/// deep.
pub open spec fn value_ok(v: Value, d: nat) -> bool
    decreases v,
{
    match v {
        Value::Array(xs) => d >= 1 && forall|i: int|
            0 <= i < xs@.len() ==> value_ok(#[trigger] xs@[i], (d - 1) as nat),
        Value::Object(ms) => d >= 1 && forall|i: int|
            0 <= i < ms@.len() ==> value_ok((#[trigger] ms@[i]).value, (d - 1) as nat),
        _ => true,
    }
}

pub open spec fn members_ok(ms: Seq<Member>, d: nat) -> bool {
    forall|i: int| 0 <= i < ms.len() ==> value_ok((#[trigger] ms[i]).value, d)
}

spec fn result_ok(r: Result<(Value, usize), Fail>, pos: int, len: int, d: nat) -> bool {
    match r {
        Ok((v, p)) => pos < p <= len && value_ok(v, d),
        Err(f) => f.pos <= len,
    }
}

proof fn lemma_len(s: &[u8])
    ensures
        s@.len() <= usize::MAX,
{
    assert(vstd::slice::spec_slice_len(s) == s@.len());
}

// ---- UTF-8 validation, equal to vstd's `valid_utf8` (RFC 3629: no overlongs, no surrogates) ------

proof fn lemma_w2(b0: u8, b1: u8)
    requires
        0xc0 <= b0 <= 0xdf,
        0x80 <= b1 <= 0xbf,
    ensures
        (0x80 <= codepoint_width_2(b0, b1)) == (b0 >= 0xc2),
        codepoint_width_2(b0, b1) < 0x800,
{
    assert(((((b0 & 0x1Fu8) as u32) << 6u32) | ((b1 & 0x3fu8) as u32) >= 0x80u32) == (b0 >= 0xc2u8)
        && ((((b0 & 0x1Fu8) as u32) << 6u32) | ((b1 & 0x3fu8) as u32)) < 0x800u32) by (bit_vector)
        requires
            0xc0u8 <= b0 <= 0xdfu8,
            0x80u8 <= b1 <= 0xbfu8,
    ;
}

proof fn lemma_w3(b0: u8, b1: u8, b2: u8)
    requires
        0xe0 <= b0 <= 0xef,
        0x80 <= b1 <= 0xbf,
        0x80 <= b2 <= 0xbf,
    ensures
        (0x800 <= codepoint_width_3(b0, b1, b2)) == (b0 != 0xe0 || b1 >= 0xa0),
        (0xD800 <= codepoint_width_3(b0, b1, b2) <= 0xDFFF) == (b0 == 0xed && b1 >= 0xa0),
{
    let cp = codepoint_width_3(b0, b1, b2);
    assert(cp == (((b0 & 0x0Fu8) as u32) << 12u32) | (((b1 & 0x3fu8) as u32) << 6u32) | ((b2
        & 0x3fu8) as u32));
    assert((0x800u32 <= cp) == (b0 != 0xe0u8 || b1 >= 0xa0u8) && (0xD800u32 <= cp && cp <= 0xDFFFu32)
        == (b0 == 0xedu8 && b1 >= 0xa0u8)) by (bit_vector)
        requires
            0xe0u8 <= b0 <= 0xefu8,
            0x80u8 <= b1 <= 0xbfu8,
            0x80u8 <= b2 <= 0xbfu8,
            cp == (((b0 & 0x0Fu8) as u32) << 12u32) | (((b1 & 0x3fu8) as u32) << 6u32) | ((b2
                & 0x3fu8) as u32),
    ;
}

proof fn lemma_w4(b0: u8, b1: u8, b2: u8, b3: u8)
    requires
        0xf0 <= b0 <= 0xf7,
        0x80 <= b1 <= 0xbf,
        0x80 <= b2 <= 0xbf,
        0x80 <= b3 <= 0xbf,
    ensures
        (0x10000 <= codepoint_width_4(b0, b1, b2, b3) <= 0x10ffff) == ((b0 == 0xf0 && b1 >= 0x90) || (
        0xf1 <= b0 <= 0xf3) || (b0 == 0xf4 && b1 <= 0x8f)),
{
    let cp = codepoint_width_4(b0, b1, b2, b3);
    assert(cp == (((b0 & 0x07u8) as u32) << 18u32) | (((b1 & 0x3fu8) as u32) << 12u32) | (((b2
        & 0x3fu8) as u32) << 6u32) | ((b3 & 0x3fu8) as u32));
    assert((0x10000u32 <= cp && cp <= 0x10ffffu32) == ((b0 == 0xf0u8 && b1 >= 0x90u8) || (0xf1u8 <= b0
        && b0 <= 0xf3u8) || (b0 == 0xf4u8 && b1 <= 0x8fu8))) by (bit_vector)
        requires
            0xf0u8 <= b0 <= 0xf7u8,
            0x80u8 <= b1 <= 0xbfu8,
            0x80u8 <= b2 <= 0xbfu8,
            0x80u8 <= b3 <= 0xbfu8,
            cp == (((b0 & 0x07u8) as u32) << 18u32) | (((b1 & 0x3fu8) as u32) << 12u32) | (((b2
                & 0x3fu8) as u32) << 6u32) | ((b3 & 0x3fu8) as u32),
    ;
}

} // verus!

/// Length of the well-formed scalar starting at `s[i]`, 0 if there is none.
#[cfg_attr(verus_keep_ghost, verus_spec(n =>
    requires
        i < s@.len(),
    ensures
        n <= s@.len() - i,
        n == 0 <==> !valid_first_scalar(s@.subrange(i as int, s@.len() as int)),
        n > 0 ==> n == length_of_first_scalar(s@.subrange(i as int, s@.len() as int)),
))]
fn scalar_len(s: &[u8], i: usize) -> usize {
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost t = s@.subrange(i as int, s@.len() as int); }
    let rest = s.len() - i;
    let b0 = s[i];
    if b0 < 0x80 {
        return 1;
    }
    if b0 < 0xc0 || b0 > 0xf7 {
        return 0;
    }
    if b0 < 0xe0 {
        if rest < 2 || !(0x80 <= s[i + 1] && s[i + 1] <= 0xbf) {
            return 0;
        }
        #[cfg(verus_keep_ghost)]
        proof! {
            lemma_w2(b0, t[1]);
        }
        return if b0 >= 0xc2 { 2 } else { 0 };
    }
    if b0 < 0xf0 {
        if rest < 3
            || !(0x80 <= s[i + 1] && s[i + 1] <= 0xbf)
            || !(0x80 <= s[i + 2] && s[i + 2] <= 0xbf)
        {
            return 0;
        }
        let b1 = s[i + 1];
        #[cfg(verus_keep_ghost)]
        proof! {
            lemma_w3(b0, b1, t[2]);
        }
        return if (b0 != 0xe0 || b1 >= 0xa0) && !(b0 == 0xed && b1 >= 0xa0) {
            3
        } else {
            0
        };
    }
    if rest < 4
        || !(0x80 <= s[i + 1] && s[i + 1] <= 0xbf)
        || !(0x80 <= s[i + 2] && s[i + 2] <= 0xbf)
        || !(0x80 <= s[i + 3] && s[i + 3] <= 0xbf)
    {
        return 0;
    }
    let b1 = s[i + 1];
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_w4(b0, b1, t[2], t[3]);
    }
    if (b0 == 0xf0 && b1 >= 0x90) || (0xf1 <= b0 && b0 <= 0xf3) || (b0 == 0xf4 && b1 <= 0x8f) {
        4
    } else {
        0
    }
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r == valid_utf8(s@),
))]
pub fn utf8_valid(s: &[u8]) -> bool {
    let mut i: usize = 0;
    #[cfg(verus_keep_ghost)]
    proof! { assert(s@.subrange(0, s@.len() as int) =~= s@); }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            i <= s@.len(),
            valid_utf8(s@) == valid_utf8(s@.subrange(i as int, s@.len() as int)),
        decreases s.len() - i,
    ))]
    while i < s.len() {
        let n = scalar_len(s, i);
        if n == 0 {
            return false;
        }
        #[cfg(verus_keep_ghost)]
        proof! {
            let t = s@.subrange(i as int, s@.len() as int);
            assert(pop_first_scalar(t) =~= s@.subrange(i + n, s@.len() as int));
        }
        i += n;
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(s@.subrange(i as int, s@.len() as int) =~= Seq::<u8>::empty());
    }
    true
}

/// SAFETY: `bytes` is valid UTF-8: `requires` makes Verus prove `valid_utf8(bytes@)` (vstd's RFC 3629 definition) at
/// every call, from the parser's validator, which is proven equal to it. Trusted: that this definition is the one
/// `String` requires.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        valid_utf8(bytes@),
))]
fn utf8_string(bytes: Vec<u8>) -> String {
    unsafe { String::from_utf8_unchecked(bytes) }
}

#[cfg_attr(verus_keep_ghost, verifier::external_body)]
fn owned(msg: &str) -> String {
    String::from(msg)
}

/// The bytes of a member's key (trusted accessor; the duplicate-key pass only needs it to be some slice).
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
fn key_bytes(m: &Member) -> &[u8] {
    m.key.as_bytes()
}

// ---- trusted float boundary ----------------------------------------------------------------------

/// The value of a lexeme that matched the JSON number grammar, if it is within json.zig's
/// `max_abs_number` (9e15). Floats are outside Verus's model: correctly rounded decimal-to-binary
/// conversion is core's `f64::from_str` (as Zig's `std.fmt.parseFloat`), trusted, not proven.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
#[cfg_attr(verus_keep_ghost, verus_spec)]
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

#[cfg_attr(verus_keep_ghost, verus_spec)]
fn is_ws(c: u8) -> bool {
    c == 32 || c == 9 || c == 10 || c == 13
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos <= src@.len(),
    ensures
        pos <= r <= src@.len(),
))]
fn ws(src: &[u8], pos: usize) -> usize {
    let mut p = pos;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            pos <= p <= src@.len(),
        decreases src.len() - p,
    ))]
    while p < src.len() && is_ws(src[p]) {
        p += 1;
    }
    p
}

#[cfg_attr(verus_keep_ghost, verus_spec)]
fn is_digit(c: u8) -> bool {
    48 <= c && c <= 57
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos <= src@.len(),
    ensures
        r ==> pos + word@.len() <= src@.len(),
))]
fn starts_with(src: &[u8], pos: usize, word: &[u8]) -> bool {
    if src.len() - pos < word.len() {
        return false;
    }
    let mut k: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            k <= word@.len(),
            pos + word@.len() <= src@.len(),
        decreases word.len() - k,
    ))]
    while k < word.len() {
        if src[pos + k] != word[k] {
            return false;
        }
        k += 1;
    }
    true
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        a <= b <= src@.len(),
    ensures
        r@ == src@.subrange(a as int, b as int),
))]
fn slice_to_vec(src: &[u8], a: usize, b: usize) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(&src[a..b]);
    #[cfg(verus_keep_ghost)]
    proof! { assert(v@ =~= src@.subrange(a as int, b as int)); }
    v
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos <= src@.len(),
        depth <= MAX_DEPTH,
    ensures
        result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    decreases src@.len() - pos, 1nat,
))]
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

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos <= src@.len(),
    ensures
        pos <= r <= src@.len(),
))]
fn digits(src: &[u8], pos: usize) -> usize {
    let mut p = pos;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            pos <= p <= src@.len(),
        decreases src.len() - p,
    ))]
    while p < src.len() && is_digit(src[p]) {
        p += 1;
    }
    p
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos < src@.len(),
    ensures
        match r {
            Ok((_, p)) => pos < p <= src@.len(),
            Err(f) => f.pos <= src@.len(),
        },
))]
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

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos <= src@.len(),
    ensures
        match r {
            Ok(v) => pos + 4 <= src@.len() && v <= 0xffff,
            Err(f) => f.pos == pos,
        },
))]
fn hex4(src: &[u8], pos: usize) -> Result<u32, Fail> {
    if src.len() - pos < 4 {
        return Err(Fail {
            pos,
            msg: "truncated \\u escape",
        });
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_len(src);
    }
    let mut v: u32 = 0;
    let mut k: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            k <= 4,
            pos + 4 <= src@.len() <= usize::MAX,
            v < pow16(k as nat),
            pow16(k as nat) <= 0x10000,
        decreases 4 - k,
    ))]
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
        #[cfg(verus_keep_ghost)]
        proof! { assert(pow16(0) == 1 && pow16(1) == 16 && pow16(2) == 256 && pow16(3) == 4096 && pow16(4) == 0x10000) by (compute); }
        #[cfg(verus_keep_ghost)]
        proof! { assert(v * 16 + d < pow16((k + 1) as nat)) by (nonlinear_arith)
            requires
                v < pow16(k as nat),
                d < 16,
                pow16((k + 1) as nat) == 16 * pow16(k as nat),
        ; }
        v = v * 16 + d;
        k += 1;
    }
    #[cfg(verus_keep_ghost)]
    proof! { assert(pow16(4) == 0x10000) by (compute); }
    Ok(v)
}

#[cfg(verus_keep_ghost)]
verus! {

pub open spec fn pow16(k: nat) -> nat
    decreases k,
{
    if k == 0 {
        1
    } else {
        16 * pow16((k - 1) as nat)
    }
}

} // verus!

/// UTF-8 bytes of a scalar value (`std.unicode.utf8Encode`).
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        cp <= 0x10ffff,
))]
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

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos < src@.len(),
    ensures
        match r {
            Ok((s, p)) => pos < p <= src@.len(),
            Err(f) => f.pos <= src@.len(),
        },
))]
fn parse_string(src: &[u8], pos: usize) -> Result<(String, usize), Fail> {
    let start = pos + 1;
    let mut p = start;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            start <= p <= src@.len(),
            start == pos + 1,
        decreases src.len() - p,
    ))]
    while p < src.len() {
        let c = src[p];
        if c == 34 {
            let s = slice_to_vec(src, start, p);
            if !utf8_valid(s.as_slice()) {
                return Err(Fail {
                    pos: p,
                    msg: "string is not valid UTF-8",
                });
            }
            return Ok((utf8_string(s), p + 1));
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
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            start <= p <= src@.len(),
            start == pos + 1,
        decreases src.len() - p,
    ))]
    while p < src.len() {
        let c = src[p];
        if c == 34 {
            if !utf8_valid(buf.as_slice()) {
                return Err(Fail {
                    pos: p,
                    msg: "string is not valid UTF-8",
                });
            }
            return Ok((utf8_string(buf), p + 1));
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
                        #[cfg(verus_keep_ghost)]
                        proof! { assert(t << 10u32 <= 0xffc00u32) by (bit_vector)
                            requires
                                t < 0x400u32,
                        ; }
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

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos < src@.len(),
        depth <= MAX_DEPTH,
    ensures
        result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    decreases src@.len() - pos, 0nat,
))]
fn array(src: &[u8], pos: usize, depth: u32) -> Result<(Value, usize), Fail> {
    if depth >= MAX_DEPTH {
        return Err(Fail {
            pos,
            msg: "nesting too deep",
        });
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_len(src);
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost d = (MAX_DEPTH - depth) as nat; }
    let mut items: Vec<Value> = Vec::new();
    let mut p = ws(src, pos + 1);
    if p < src.len() && src[p] == 93 {
        return Ok((Value::Array(items), p + 1));
    }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            pos < p <= src@.len(),
            depth < MAX_DEPTH,
            d == (MAX_DEPTH - depth) as nat,
            forall|i: int| 0 <= i < items@.len() ==> value_ok(#[trigger] items@[i], (d - 1) as nat),
        decreases src@.len() - p,
    ))]
    loop {
        p = ws(src, p);
        match value(src, p, depth + 1) {
            Ok((v, q)) => {
                #[cfg(verus_keep_ghost)]
                proof! { assert(value_ok(v, (d - 1) as nat)); }
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
            #[cfg(verus_keep_ghost)]
            proof! { assert(value_ok(Value::Array(items), d)); }
            return Ok((Value::Array(items), p + 1));
        } else {
            return Err(Fail {
                pos: p,
                msg: "expected ',' or ']' in array",
            });
        }
    }
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        pos < src@.len(),
        depth <= MAX_DEPTH,
    ensures
        result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    decreases src@.len() - pos, 0nat,
))]
fn object(src: &[u8], pos: usize, depth: u32) -> Result<(Value, usize), Fail> {
    if depth >= MAX_DEPTH {
        return Err(Fail {
            pos,
            msg: "nesting too deep",
        });
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_len(src);
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost d = (MAX_DEPTH - depth) as nat; }
    let mut items: Vec<Member> = Vec::new();
    let mut p = ws(src, pos + 1);
    if p < src.len() && src[p] == 125 {
        return Ok((Value::Object(items), p + 1));
    }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            pos < p <= src@.len(),
            depth < MAX_DEPTH,
            d == (MAX_DEPTH - depth) as nat,
            members_ok(items@, (d - 1) as nat),
        decreases src@.len() - p,
    ))]
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
                #[cfg(verus_keep_ghost)]
                proof! { assert(value_ok(v, (d - 1) as nat)); }
                #[cfg(verus_keep_ghost)]
                proof_decl! { let ghost before = items@; }
                items.push(Member { key: k, value: v });
                #[cfg(verus_keep_ghost)]
                proof! { assert(items@[items@.len() - 1].key@ == k@); }
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
            #[cfg(verus_keep_ghost)]
            proof_decl! { let ghost its = items@; }
            let ms = dedupe(items);
            #[cfg(verus_keep_ghost)]
            proof! { assert(members_ok(its, (d - 1) as nat)); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(members_ok(ms@, (d - 1) as nat)); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(value_ok(Value::Object(ms), d)); }
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

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r == (a@ == b@),
))]
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut k: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            k <= a@.len() == b@.len(),
            a@.subrange(0, k as int) == b@.subrange(0, k as int),
        decreases a.len() - k,
    ))]
    while k < a.len() {
        if a[k] != b[k] {
            return false;
        }
        k += 1;
        #[cfg(verus_keep_ghost)]
        proof! { assert(a@.subrange(0, k as int) =~= a@.subrange(0, k - 1).push(a@[k - 1])); }
        #[cfg(verus_keep_ghost)]
        proof! { assert(b@.subrange(0, k as int) =~= b@.subrange(0, k - 1).push(b@[k - 1])); }
    }
    #[cfg(verus_keep_ghost)]
    proof! { assert(a@ =~= a@.subrange(0, k as int)); }
    #[cfg(verus_keep_ghost)]
    proof! { assert(b@ =~= b@.subrange(0, k as int)); }
    true
}

/// `std.mem.order(u8, a, b) == .lt`
#[cfg_attr(verus_keep_ghost, verus_spec)]
fn bytes_lt(a: &[u8], b: &[u8]) -> bool {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut k: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            k <= n <= a@.len(),
            n <= b@.len(),
        decreases n - k,
    ))]
    while k < n {
        if a[k] != b[k] {
            return a[k] < b[k];
        }
        k += 1;
    }
    a.len() < b.len()
}

/// json.zig `keyLess`: by key, then by position (so the sort order is total).
#[cfg_attr(verus_keep_ghost, verus_spec)]
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

#[cfg(verus_keep_ghost)]
verus! {

pub open spec fn keeps_ok(a: Seq<Member>, b: Seq<Member>) -> bool {
    forall|d: nat| members_ok(a, d) ==> #[trigger] members_ok(b, d)
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        keeps_ok(items@, r@),
))]
fn dedupe(items: Vec<Member>) -> Vec<Member> {
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost items0 = items@; }
    let mut items = items;
    let n = items.len();
    if n < 2 {
        return items;
    }
    if n <= 16 {
        let mut dup = false;
        let mut i: usize = 1;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                n == items@.len(),
                1 <= i <= n,
            decreases n - i,
        ))]
        while i < n && !dup {
            let mut j: usize = 0;
            #[cfg_attr(verus_keep_ghost, verus_spec(
                invariant
                    n == items@.len(),
                    i < n,
                    j <= i,
                decreases i - j,
            ))]
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
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            i <= n,
            idx@.len() == i,
            forall|k: int| 0 <= k < i ==> #[trigger] idx@[k] == k,
        decreases n - i,
    ))]
    while i < n {
        idx.push(i);
        i += 1;
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost idx0 = idx@; }
    {
        let its = &items;
        crate::sort::stable(idx.as_mut_slice(), |x: &usize, y: &usize| {
            key_less(its, *x, *y)
        });
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        let (g, inv) = choose|g: Seq<int>, inv: Seq<int>|
            {
                &&& #[trigger] is_perm(g, inv, 0, n as int)
                &&& tracks(idx@, idx0, g, 0, n as int)
                &&& true
            };
        assert forall|k: int| 0 <= k < n implies #[trigger] idx@[k] < n by {
            assert(idx@[k] == idx0[g[k]]);
        }
    }
    let mut drop: Vec<bool> = Vec::with_capacity(n);
    let mut i: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            i <= n,
            drop@.len() == i,
        decreases n - i,
    ))]
    while i < n {
        drop.push(false);
        i += 1;
    }
    let mut has_dup = false;
    let mut g: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            g <= n == items@.len() == idx@.len() == drop@.len(),
            forall|k: int| 0 <= k < n ==> #[trigger] idx@[k] < n,
            keeps_ok(items0, items@),
        decreases n - g,
    ))]
    while g < n {
        let mut e = g + 1;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                g < e <= n == items@.len() == idx@.len(),
                forall|k: int| 0 <= k < n ==> #[trigger] idx@[k] < n,
            decreases n - e,
        ))]
        while e < n && bytes_eq(key_bytes(&items[idx[e]]), key_bytes(&items[idx[g]])) {
            e += 1;
        }
        if e - g > 1 {
            has_dup = true;
            #[cfg(verus_keep_ghost)]
            proof_decl! { let ghost before = items@; }
            let j = idx[e - 1];
            let i = idx[g];
            let mut last = Value::Null;
            core::mem::swap(&mut items[j].value, &mut last);
            #[cfg(verus_keep_ghost)]
            proof_decl! { let ghost mid = items@; }
            #[cfg(verus_keep_ghost)]
            proof! { assert(mid.len() == before.len()); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(mid[j as int].key == before[j as int].key); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(forall|t: int| 0 <= t < n && t != j ==> #[trigger] mid[t] == before[t]); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(last == before[j as int].value); }
            items[i].value = last;
            #[cfg(verus_keep_ghost)]
            proof! { assert(items@.len() == mid.len()); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(items@[i as int].key == mid[i as int].key); }
            #[cfg(verus_keep_ghost)]
            proof! { assert(forall|t: int| 0 <= t < n && t != i ==> #[trigger] items@[t] == mid[t]); }
            #[cfg(verus_keep_ghost)]
            proof! {
                assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(items@, dd) by {
                    assert(members_ok(before, dd));
                    assert forall|t: int| 0 <= t < n implies value_ok((#[trigger] items@[t]).value,
                        dd,
                    ) by {
                        assert(items@[t].key == before[t].key);
                        assert(value_ok(before[t].value, dd));
                        assert(value_ok(before[j as int].value, dd));
                        if t != i && t != j {
                            assert(items@[t] == before[t]);
                        } else if t == i {
                            assert(items@[t].value == before[j as int].value);
                        }
                    }
                }
            }
            let mut k = g + 1;
            #[cfg_attr(verus_keep_ghost, verus_spec(
                invariant
                    g < k <= e <= n == idx@.len() == drop@.len(),
                    forall|k: int| 0 <= k < n ==> #[trigger] idx@[k] < n,
                decreases e - k,
            ))]
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
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            items@.len() + rev@.len() == n,
            keeps_ok(items0, items@),
            keeps_ok(items0, rev@),
        decreases items@.len(),
    ))]
    while items.len() > 0 {
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost bi = items@;
            let ghost br = rev@;
        }
        match items.pop() {
            Some(m) => rev.push(m),
            None => {}
        }
        #[cfg(verus_keep_ghost)]
        proof! {
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(rev@, dd) by {
                assert(members_ok(bi, dd) && members_ok(br, dd));
                assert forall|t: int| 0 <= t < rev@.len() implies value_ok((#[trigger] rev@[t]).value, dd) by {
                    if t == br.len() {
                        assert(rev@[t] == bi[bi.len() - 1]);
                    } else {
                        assert(rev@[t] == br[t]);
                    }
                }
            }
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(items@, dd) by {
                assert(members_ok(bi, dd) && members_ok(br, dd));
                assert forall|t: int| 0 <= t < rev@.len() implies value_ok((#[trigger] rev@[t]).value, dd) by {
                    if t == br.len() {
                        assert(rev@[t] == bi[bi.len() - 1]);
                    } else {
                        assert(rev@[t] == br[t]);
                    }
                }
                assert forall|t: int| 0 <= t < items@.len() implies value_ok((#[trigger] items@[t]).value, dd) by {
                    assert(items@[t] == bi[t]);
                }
            }
        }
    }
    let mut i: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            i + rev@.len() == n == drop@.len(),
            keeps_ok(items0, rev@),
            keeps_ok(items0, out@),
        decreases rev@.len(),
    ))]
    while rev.len() > 0 {
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost br = rev@;
            let ghost bo = out@;
        }
        match rev.pop() {
            Some(m) => {
                if !drop[i] {
                    out.push(m);
                }
            }
            None => {}
        }
        #[cfg(verus_keep_ghost)]
        proof! {
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(rev@, dd) by {
                assert(members_ok(br, dd));
                assert forall|t: int| 0 <= t < rev@.len() implies value_ok((#[trigger] rev@[t]).value, dd) by {
                    assert(rev@[t] == br[t]);
                }
            }
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(out@, dd) by {
                assert(members_ok(br, dd) && members_ok(bo, dd));
                assert forall|t: int| 0 <= t < out@.len() implies value_ok((#[trigger] out@[t]).value, dd) by {
                    if t == bo.len() {
                        assert(out@[t] == br[br.len() - 1]);
                    } else {
                        assert(out@[t] == bo[t]);
                    }
                }
                assert forall|t: int| 0 <= t < rev@.len() implies value_ok((#[trigger] rev@[t]).value, dd) by {
                    assert(rev@[t] == br[t]);
                }
            }
        }
        i += 1;
    }
    out
}

// ---- entry point --------------------------------------------------------------------------------

#[cfg_attr(verus_keep_ghost, verus_spec)]
fn location(src: &[u8], pos: usize) -> (u32, u32) {
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    let end = if pos < src.len() { pos } else { src.len() };
    let mut k: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            k <= end <= src@.len(),
        decreases end - k,
    ))]
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
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        match r {
            Ok(v) => value_ok(v, MAX_DEPTH as nat),
            Err(_) => true,
        },
))]
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
        while frac.is_multiple_of(10) {
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
