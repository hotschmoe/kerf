use crate::sort::*;
use alloc::vec::Vec;
use vstd::prelude::*;
use vstd::utf8::*;

verus! {

/// Deepest array/object nesting the parser accepts (json.zig).
pub const MAX_DEPTH: u32 = 200;

pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u8>),
    Array(Vec<Value>),
    Object(Vec<Member>),
}

pub struct Member {
    pub key: Vec<u8>,
    pub value: Value,
}

/// A syntax error at 1-based `line` and `col`, with json.zig's message.
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: &'static str,
}

struct Fail {
    pos: usize,
    msg: &'static str,
}

/// Every string (and object key) in `v` is valid UTF-8, and arrays and objects nest at most `d`
/// deep.
pub open spec fn value_ok(v: Value, d: nat) -> bool
    decreases v,
{
    match v {
        Value::String(s) => valid_utf8(s@),
        Value::Array(xs) => d >= 1 && forall|i: int|
            0 <= i < xs@.len() ==> value_ok(#[trigger] xs@[i], (d - 1) as nat),
        Value::Object(ms) => d >= 1 && forall|i: int|
            0 <= i < ms@.len() ==> valid_utf8((#[trigger] ms@[i]).key@) && value_ok(ms@[i].value, (d - 1) as nat),
        _ => true,
    }
}

pub open spec fn members_ok(ms: Seq<Member>, d: nat) -> bool {
    forall|i: int| 0 <= i < ms.len() ==> valid_utf8((#[trigger] ms[i]).key@) && value_ok(ms[i].value, d)
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

/// Length of the well-formed scalar starting at `s[i]`, 0 if there is none.
fn scalar_len(s: &[u8], i: usize) -> (n: usize)
    requires
        i < s@.len(),
    ensures
        n <= s@.len() - i,
        n == 0 <==> !valid_first_scalar(s@.subrange(i as int, s@.len() as int)),
        n > 0 ==> n == length_of_first_scalar(s@.subrange(i as int, s@.len() as int)),
{
    let ghost t = s@.subrange(i as int, s@.len() as int);
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
        proof {
            lemma_w2(b0, t[1]);
        }
        return if b0 >= 0xc2 { 2 } else { 0 };
    }
    if b0 < 0xf0 {
        if rest < 3 || !(0x80 <= s[i + 1] && s[i + 1] <= 0xbf) || !(0x80 <= s[i + 2] && s[i + 2] <= 0xbf) {
            return 0;
        }
        let b1 = s[i + 1];
        proof {
            lemma_w3(b0, b1, t[2]);
        }
        return if (b0 != 0xe0 || b1 >= 0xa0) && !(b0 == 0xed && b1 >= 0xa0) { 3 } else { 0 };
    }
    if rest < 4 || !(0x80 <= s[i + 1] && s[i + 1] <= 0xbf) || !(0x80 <= s[i + 2] && s[i + 2] <= 0xbf)
        || !(0x80 <= s[i + 3] && s[i + 3] <= 0xbf) {
        return 0;
    }
    let b1 = s[i + 1];
    proof {
        lemma_w4(b0, b1, t[2], t[3]);
    }
    if (b0 == 0xf0 && b1 >= 0x90) || (0xf1 <= b0 && b0 <= 0xf3) || (b0 == 0xf4 && b1 <= 0x8f) {
        4
    } else {
        0
    }
}

pub fn utf8_valid(s: &[u8]) -> (r: bool)
    ensures
        r == valid_utf8(s@),
{
    let mut i: usize = 0;
    assert(s@.subrange(0, s@.len() as int) =~= s@);
    while i < s.len()
        invariant
            i <= s@.len(),
            valid_utf8(s@) == valid_utf8(s@.subrange(i as int, s@.len() as int)),
        decreases s.len() - i,
    {
        let n = scalar_len(s, i);
        if n == 0 {
            return false;
        }
        proof {
            let t = s@.subrange(i as int, s@.len() as int);
            assert(pop_first_scalar(t) =~= s@.subrange(i + n, s@.len() as int));
        }
        i += n;
    }
    proof {
        assert(s@.subrange(i as int, s@.len() as int) =~= Seq::<u8>::empty());
    }
    true
}

// ---- trusted float boundary ----------------------------------------------------------------------

/// The value of a lexeme that matched the JSON number grammar, if it is within json.zig's
/// `max_abs_number` (9e15). Floats are outside Verus's model: correctly rounded decimal-to-binary
/// conversion is core's `f64::from_str` (as Zig's `std.fmt.parseFloat`), trusted, not proven.
#[verifier::external_body]
fn number_value(lexeme: &[u8]) -> (r: Result<f64, bool>) {
    let Ok(text) = core::str::from_utf8(lexeme) else { return Err(false) };
    let Ok(f) = text.parse::<f64>() else { return Err(false) };
    if f.abs() <= 9.0e15 { Ok(f) } else { Err(true) }
}

// ---- parser (json.zig `Parser`, same acceptance, values and error positions) ---------------------

fn is_ws(c: u8) -> bool {
    c == 32 || c == 9 || c == 10 || c == 13
}

fn ws(src: &[u8], pos: usize) -> (r: usize)
    requires
        pos <= src@.len(),
    ensures
        pos <= r <= src@.len(),
{
    let mut p = pos;
    while p < src.len() && is_ws(src[p])
        invariant
            pos <= p <= src@.len(),
        decreases src.len() - p,
    {
        p += 1;
    }
    p
}

fn is_digit(c: u8) -> bool {
    48 <= c && c <= 57
}

fn starts_with(src: &[u8], pos: usize, word: &[u8]) -> (r: bool)
    requires
        pos <= src@.len(),
    ensures
        r ==> pos + word@.len() <= src@.len(),
{
    if src.len() - pos < word.len() {
        return false;
    }
    let mut k: usize = 0;
    while k < word.len()
        invariant
            k <= word@.len(),
            pos + word@.len() <= src@.len(),
        decreases word.len() - k,
    {
        if src[pos + k] != word[k] {
            return false;
        }
        k += 1;
    }
    true
}

fn slice_to_vec(src: &[u8], a: usize, b: usize) -> (r: Vec<u8>)
    requires
        a <= b <= src@.len(),
    ensures
        r@ == src@.subrange(a as int, b as int),
{
    let mut v: Vec<u8> = Vec::with_capacity(b - a);
    let mut k = a;
    while k < b
        invariant
            a <= k <= b <= src@.len(),
            v@ == src@.subrange(a as int, k as int),
        decreases b - k,
    {
        v.push(src[k]);
        k += 1;
        assert(v@ =~= src@.subrange(a as int, k as int));
    }
    v
}

fn value(src: &[u8], pos: usize, depth: u32) -> (r: Result<(Value, usize), Fail>)
    requires
        pos <= src@.len(),
        depth <= MAX_DEPTH,
    ensures
        result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    decreases src@.len() - pos, 1nat,
{
    if pos >= src.len() {
        return Err(Fail { pos, msg: "unexpected end of input" });
    }
    let c = src[pos];
    if c == 123 {
        object(src, pos, depth)
    } else if c == 91 {
        array(src, pos, depth)
    } else if c == 34 {
        match string(src, pos) {
            Ok((s, p)) => Ok((Value::String(s), p)),
            Err(f) => Err(f),
        }
    } else if c == 116 {
        if starts_with(src, pos, &[116u8, 114, 117, 101]) {
            Ok((Value::Bool(true), pos + 4))
        } else {
            Err(Fail { pos, msg: "invalid literal" })
        }
    } else if c == 102 {
        if starts_with(src, pos, &[102u8, 97, 108, 115, 101]) {
            Ok((Value::Bool(false), pos + 5))
        } else {
            Err(Fail { pos, msg: "invalid literal" })
        }
    } else if c == 110 {
        if starts_with(src, pos, &[110u8, 117, 108, 108]) {
            Ok((Value::Null, pos + 4))
        } else {
            Err(Fail { pos, msg: "invalid literal" })
        }
    } else if c == 45 || is_digit(c) {
        match number(src, pos) {
            Ok((f, p)) => Ok((Value::Number(f), p)),
            Err(f) => Err(f),
        }
    } else {
        Err(Fail { pos, msg: "unexpected character; expected a JSON value" })
    }
}

fn digits(src: &[u8], pos: usize) -> (r: usize)
    requires
        pos <= src@.len(),
    ensures
        pos <= r <= src@.len(),
{
    let mut p = pos;
    while p < src.len() && is_digit(src[p])
        invariant
            pos <= p <= src@.len(),
        decreases src.len() - p,
    {
        p += 1;
    }
    p
}

fn number(src: &[u8], pos: usize) -> (r: Result<(f64, usize), Fail>)
    requires
        pos < src@.len(),
    ensures
        match r {
            Ok((_, p)) => pos < p <= src@.len(),
            Err(f) => f.pos <= src@.len(),
        },
{
    let start = pos;
    let mut p = pos;
    if src[p] == 45 {
        p += 1;
    }
    let first = p;
    p = digits(src, p);
    if p == first {
        return Err(Fail { pos: p, msg: "invalid number" });
    }
    if p - first > 1 && src[first] == 48 {
        return Err(Fail { pos: p, msg: "invalid number: leading zeros are not allowed (write 7, not 07)" });
    }
    if p < src.len() && src[p] == 46 {
        p += 1;
        let f = p;
        p = digits(src, p);
        if p == f {
            return Err(Fail { pos: p, msg: "invalid number: digits expected after '.'" });
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
            return Err(Fail { pos: p, msg: "invalid number exponent" });
        }
    }
    let lexeme = slice_to_vec(src, start, p);
    match number_value(lexeme.as_slice()) {
        Ok(f) => Ok((f, p)),
        Err(false) => Err(Fail { pos: p, msg: "invalid number" }),
        Err(true) => Err(Fail {
            pos: p,
            msg: "number out of range: a Kerf number must be finite and at most 9e15 in magnitude",
        }),
    }
}

fn hex4(src: &[u8], pos: usize) -> (r: Result<u32, Fail>)
    requires
        pos <= src@.len(),
    ensures
        match r {
            Ok(v) => pos + 4 <= src@.len() && v <= 0xffff,
            Err(f) => f.pos == pos,
        },
{
    if src.len() - pos < 4 {
        return Err(Fail { pos, msg: "truncated \\u escape" });
    }
    proof {
        lemma_len(src);
    }
    let mut v: u32 = 0;
    let mut k: usize = 0;
    while k < 4
        invariant
            k <= 4,
            pos + 4 <= src@.len() <= usize::MAX,
            v < pow16(k as nat),
            pow16(k as nat) <= 0x10000,
        decreases 4 - k,
    {
        let c = src[pos + k];
        let d: u32 = if 48 <= c && c <= 57 {
            (c - 48) as u32
        } else if 97 <= c && c <= 102 {
            (c - 97 + 10) as u32
        } else if 65 <= c && c <= 70 {
            (c - 65 + 10) as u32
        } else {
            return Err(Fail { pos, msg: "invalid \\u escape" });
        };
        assert(pow16(0) == 1 && pow16(1) == 16 && pow16(2) == 256 && pow16(3) == 4096 && pow16(4) == 0x10000) by (compute);
        assert(v * 16 + d < pow16((k + 1) as nat)) by (nonlinear_arith)
            requires
                v < pow16(k as nat),
                d < 16,
                pow16((k + 1) as nat) == 16 * pow16(k as nat),
        ;
        v = v * 16 + d;
        k += 1;
    }
    assert(pow16(4) == 0x10000) by (compute);
    Ok(v)
}

pub open spec fn pow16(k: nat) -> nat
    decreases k,
{
    if k == 0 {
        1
    } else {
        16 * pow16((k - 1) as nat)
    }
}

/// UTF-8 bytes of a scalar value (`std.unicode.utf8Encode`).
fn push_utf8(buf: &mut Vec<u8>, cp: u32)
    requires
        cp <= 0x10ffff,
{
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

fn string(src: &[u8], pos: usize) -> (r: Result<(Vec<u8>, usize), Fail>)
    requires
        pos < src@.len(),
    ensures
        match r {
            Ok((s, p)) => pos < p <= src@.len() && valid_utf8(s@),
            Err(f) => f.pos <= src@.len(),
        },
{
    let start = pos + 1;
    let mut p = start;
    while p < src.len()
        invariant
            start <= p <= src@.len(),
            start == pos + 1,
        decreases src.len() - p,
    {
        let c = src[p];
        if c == 34 {
            let s = slice_to_vec(src, start, p);
            if !utf8_valid(s.as_slice()) {
                return Err(Fail { pos: p, msg: "string is not valid UTF-8" });
            }
            return Ok((s, p + 1));
        }
        if c == 92 {
            break;
        }
        if c < 0x20 {
            return Err(Fail { pos: p, msg: "control character in string" });
        }
        p += 1;
    }
    if p >= src.len() {
        return Err(Fail { pos: p, msg: "unterminated string" });
    }
    let mut buf = slice_to_vec(src, start, p);
    while p < src.len()
        invariant
            start <= p <= src@.len(),
            start == pos + 1,
        decreases src.len() - p,
    {
        let c = src[p];
        if c == 34 {
            if !utf8_valid(buf.as_slice()) {
                return Err(Fail { pos: p, msg: "string is not valid UTF-8" });
            }
            return Ok((buf, p + 1));
        }
        if c < 0x20 {
            return Err(Fail { pos: p, msg: "control character in string" });
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
                        assert(t << 10u32 <= 0xffc00u32) by (bit_vector)
                            requires
                                t < 0x400u32,
                        ;
                        cp = 0x10000 + (t << 10) + (lo - 0xdc00);
                    } else {
                        return Err(Fail { pos: p, msg: "invalid surrogate pair" });
                    }
                } else {
                    return Err(Fail { pos: p, msg: "lone surrogate in \\u escape" });
                }
            }
            if 0xd800 <= cp && cp < 0xe000 {
                return Err(Fail { pos: p, msg: "invalid code point" });
            }
            push_utf8(&mut buf, cp);
        } else {
            return Err(Fail { pos: p, msg: "invalid escape sequence" });
        }
    }
    Err(Fail { pos: p, msg: "unterminated string" })
}

fn array(src: &[u8], pos: usize, depth: u32) -> (r: Result<(Value, usize), Fail>)
    requires
        pos < src@.len(),
        depth <= MAX_DEPTH,
    ensures
        result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    decreases src@.len() - pos, 0nat,
{
    if depth >= MAX_DEPTH {
        return Err(Fail { pos, msg: "nesting too deep" });
    }
    proof {
        lemma_len(src);
    }
    let ghost d = (MAX_DEPTH - depth) as nat;
    let mut items: Vec<Value> = Vec::new();
    let mut p = ws(src, pos + 1);
    if p < src.len() && src[p] == 93 {
        return Ok((Value::Array(items), p + 1));
    }
    loop
        invariant
            pos < p <= src@.len(),
            depth < MAX_DEPTH,
            d == (MAX_DEPTH - depth) as nat,
            forall|i: int| 0 <= i < items@.len() ==> value_ok(#[trigger] items@[i], (d - 1) as nat),
        decreases src@.len() - p,
    {
        p = ws(src, p);
        match value(src, p, depth + 1) {
            Ok((v, q)) => {
                assert(value_ok(v, (d - 1) as nat));
                items.push(v);
                p = ws(src, q);
            },
            Err(f) => return Err(f),
        }
        if p >= src.len() {
            return Err(Fail { pos: p, msg: "unterminated array" });
        }
        if src[p] == 44 {
            p += 1;
        } else if src[p] == 93 {
            assert(value_ok(Value::Array(items), d));
            return Ok((Value::Array(items), p + 1));
        } else {
            return Err(Fail { pos: p, msg: "expected ',' or ']' in array" });
        }
    }
}

fn object(src: &[u8], pos: usize, depth: u32) -> (r: Result<(Value, usize), Fail>)
    requires
        pos < src@.len(),
        depth <= MAX_DEPTH,
    ensures
        result_ok(r, pos as int, src@.len() as int, (MAX_DEPTH - depth) as nat),
    decreases src@.len() - pos, 0nat,
{
    if depth >= MAX_DEPTH {
        return Err(Fail { pos, msg: "nesting too deep" });
    }
    proof {
        lemma_len(src);
    }
    let ghost d = (MAX_DEPTH - depth) as nat;
    let mut items: Vec<Member> = Vec::new();
    let mut p = ws(src, pos + 1);
    if p < src.len() && src[p] == 125 {
        return Ok((Value::Object(items), p + 1));
    }
    loop
        invariant
            pos < p <= src@.len(),
            depth < MAX_DEPTH,
            d == (MAX_DEPTH - depth) as nat,
            members_ok(items@, (d - 1) as nat),
        decreases src@.len() - p,
    {
        p = ws(src, p);
        if p >= src.len() || src[p] != 34 {
            return Err(Fail { pos: p, msg: "expected a string key in object" });
        }
        let (k, q) = match string(src, p) {
            Ok(x) => x,
            Err(f) => return Err(f),
        };
        p = ws(src, q);
        if p >= src.len() || src[p] != 58 {
            return Err(Fail { pos: p, msg: "expected ':' after object key" });
        }
        p = ws(src, p + 1);
        match value(src, p, depth + 1) {
            Ok((v, q)) => {
                assert(value_ok(v, (d - 1) as nat));
                let ghost before = items@;
                items.push(Member { key: k, value: v });
                assert(items@[items@.len() - 1].key@ == k@);
                p = ws(src, q);
            },
            Err(f) => return Err(f),
        }
        if p >= src.len() {
            return Err(Fail { pos: p, msg: "unterminated object" });
        }
        if src[p] == 44 {
            p += 1;
        } else if src[p] == 125 {
            let ghost its = items@;
            let ms = dedupe(items);
            assert(members_ok(its, (d - 1) as nat));
            assert(members_ok(ms@, (d - 1) as nat));
            assert(value_ok(Value::Object(ms), d));
            return Ok((Value::Object(ms), p + 1));
        } else {
            return Err(Fail { pos: p, msg: "expected ',' or '}' in object" });
        }
    }
}

// ---- duplicate keys: the last value wins, in the position of the first occurrence ---------------

fn bytes_eq(a: &[u8], b: &[u8]) -> (r: bool)
    ensures
        r == (a@ == b@),
{
    if a.len() != b.len() {
        return false;
    }
    let mut k: usize = 0;
    while k < a.len()
        invariant
            k <= a@.len() == b@.len(),
            a@.subrange(0, k as int) == b@.subrange(0, k as int),
        decreases a.len() - k,
    {
        if a[k] != b[k] {
            return false;
        }
        k += 1;
        assert(a@.subrange(0, k as int) =~= a@.subrange(0, k - 1).push(a@[k - 1]));
        assert(b@.subrange(0, k as int) =~= b@.subrange(0, k - 1).push(b@[k - 1]));
    }
    assert(a@ =~= a@.subrange(0, k as int));
    assert(b@ =~= b@.subrange(0, k as int));
    true
}

/// `std.mem.order(u8, a, b) == .lt`
fn bytes_lt(a: &[u8], b: &[u8]) -> bool {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut k: usize = 0;
    while k < n
        invariant
            k <= n <= a@.len(),
            n <= b@.len(),
        decreases n - k,
    {
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
    let (a, b) = (items[x].key.as_slice(), items[y].key.as_slice());
    if bytes_lt(a, b) {
        true
    } else if bytes_lt(b, a) {
        false
    } else {
        x < y
    }
}

pub open spec fn keeps_ok(a: Seq<Member>, b: Seq<Member>) -> bool {
    forall|d: nat| members_ok(a, d) ==> #[trigger] members_ok(b, d)
}

fn dedupe(items: Vec<Member>) -> (r: Vec<Member>)
    ensures
        keeps_ok(items@, r@),
{
    let ghost items0 = items@;
    let mut items = items;
    let n = items.len();
    if n < 2 {
        return items;
    }
    if n <= 16 {
        let mut dup = false;
        let mut i: usize = 1;
        while i < n && !dup
            invariant
                n == items@.len(),
                1 <= i <= n,
            decreases n - i,
        {
            let mut j: usize = 0;
            while j < i && !dup
                invariant
                    n == items@.len(),
                    i < n,
                    j <= i,
                decreases i - j,
            {
                if bytes_eq(items[i].key.as_slice(), items[j].key.as_slice()) {
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
    while i < n
        invariant
            i <= n,
            idx@.len() == i,
            forall|k: int| 0 <= k < i ==> #[trigger] idx@[k] == k,
        decreases n - i,
    {
        idx.push(i);
        i += 1;
    }
    let ghost idx0 = idx@;
    {
        let its = &items;
        stable_sort_by(idx.as_mut_slice(), |x: &usize, y: &usize| key_less(its, *x, *y));
    }
    proof {
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
    while i < n
        invariant
            i <= n,
            drop@.len() == i,
        decreases n - i,
    {
        drop.push(false);
        i += 1;
    }
    let mut has_dup = false;
    let mut g: usize = 0;
    while g < n
        invariant
            g <= n == items@.len() == idx@.len() == drop@.len(),
            forall|k: int| 0 <= k < n ==> #[trigger] idx@[k] < n,
            keeps_ok(items0, items@),
        decreases n - g,
    {
        let mut e = g + 1;
        while e < n && bytes_eq(items[idx[e]].key.as_slice(), items[idx[g]].key.as_slice())
            invariant
                g < e <= n == items@.len() == idx@.len(),
                forall|k: int| 0 <= k < n ==> #[trigger] idx@[k] < n,
            decreases n - e,
        {
            e += 1;
        }
        if e - g > 1 {
            has_dup = true;
            let ghost before = items@;
            let j = idx[e - 1];
            let i = idx[g];
            let mut last = Value::Null;
            core::mem::swap(&mut items[j].value, &mut last);
            let ghost mid = items@;
            assert(mid.len() == before.len());
            assert(mid[j as int].key == before[j as int].key);
            assert(forall|t: int| 0 <= t < n && t != j ==> #[trigger] mid[t] == before[t]);
            assert(last == before[j as int].value);
            items[i].value = last;
            assert(items@.len() == mid.len());
            assert(items@[i as int].key == mid[i as int].key);
            assert(forall|t: int| 0 <= t < n && t != i ==> #[trigger] items@[t] == mid[t]);
            proof {
                assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(items@, dd) by {
                    assert(members_ok(before, dd));
                    assert forall|t: int| 0 <= t < n implies valid_utf8(#[trigger] items@[t].key@) && value_ok(
                        items@[t].value,
                        dd,
                    ) by {
                        assert(items@[t].key == before[t].key);
                        assert(valid_utf8(before[t].key@) && value_ok(before[t].value, dd));
                        assert(valid_utf8(before[j as int].key@) && value_ok(before[j as int].value, dd));
                        if t != i && t != j {
                            assert(items@[t] == before[t]);
                        } else if t == i {
                            assert(items@[t].value == before[j as int].value);
                        }
                    }
                }
            }
            let mut k = g + 1;
            while k < e
                invariant
                    g < k <= e <= n == idx@.len() == drop@.len(),
                    forall|k: int| 0 <= k < n ==> #[trigger] idx@[k] < n,
                decreases e - k,
            {
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
    while items.len() > 0
        invariant
            items@.len() + rev@.len() == n,
            keeps_ok(items0, items@),
            keeps_ok(items0, rev@),
        decreases items@.len(),
    {
        let ghost bi = items@;
        let ghost br = rev@;
        match items.pop() {
            Some(m) => rev.push(m),
            None => {},
        }
        proof {
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(rev@, dd) by {
                assert(members_ok(bi, dd) && members_ok(br, dd));
                assert forall|t: int| 0 <= t < rev@.len() implies valid_utf8(#[trigger] rev@[t].key@)
                    && value_ok(rev@[t].value, dd) by {
                    if t == br.len() {
                        assert(rev@[t] == bi[bi.len() - 1]);
                    } else {
                        assert(rev@[t] == br[t]);
                    }
                }
            }
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(items@, dd) by {
                assert(members_ok(bi, dd) && members_ok(br, dd));
                assert forall|t: int| 0 <= t < rev@.len() implies valid_utf8(#[trigger] rev@[t].key@)
                    && value_ok(rev@[t].value, dd) by {
                    if t == br.len() {
                        assert(rev@[t] == bi[bi.len() - 1]);
                    } else {
                        assert(rev@[t] == br[t]);
                    }
                }
                assert forall|t: int| 0 <= t < items@.len() implies valid_utf8(#[trigger] items@[t].key@)
                    && value_ok(items@[t].value, dd) by {
                    assert(items@[t] == bi[t]);
                }
            }
        }
    }
    let mut i: usize = 0;
    while rev.len() > 0
        invariant
            i + rev@.len() == n == drop@.len(),
            keeps_ok(items0, rev@),
            keeps_ok(items0, out@),
        decreases rev@.len(),
    {
        let ghost br = rev@;
        let ghost bo = out@;
        match rev.pop() {
            Some(m) => {
                if !drop[i] {
                    out.push(m);
                }
            },
            None => {},
        }
        proof {
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(rev@, dd) by {
                assert(members_ok(br, dd));
                assert forall|t: int| 0 <= t < rev@.len() implies valid_utf8(#[trigger] rev@[t].key@)
                    && value_ok(rev@[t].value, dd) by {
                    assert(rev@[t] == br[t]);
                }
            }
            assert forall|dd: nat| members_ok(items0, dd) implies #[trigger] members_ok(out@, dd) by {
                assert(members_ok(br, dd) && members_ok(bo, dd));
                assert forall|t: int| 0 <= t < out@.len() implies valid_utf8(#[trigger] out@[t].key@)
                    && value_ok(out@[t].value, dd) by {
                    if t == bo.len() {
                        assert(out@[t] == br[br.len() - 1]);
                    } else {
                        assert(out@[t] == bo[t]);
                    }
                }
                assert forall|t: int| 0 <= t < rev@.len() implies valid_utf8(#[trigger] rev@[t].key@)
                    && value_ok(rev@[t].value, dd) by {
                    assert(rev@[t] == br[t]);
                }
            }
        }
        i += 1;
    }
    out
}

// ---- entry point --------------------------------------------------------------------------------

fn location(src: &[u8], pos: usize) -> (r: (u32, u32)) {
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    let end = if pos < src.len() { pos } else { src.len() };
    let mut k: usize = 0;
    while k < end
        invariant
            k <= end <= src@.len(),
        decreases end - k,
    {
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
pub fn parse(src: &[u8]) -> (r: Result<Value, ParseError>)
    ensures
        match r {
            Ok(v) => value_ok(v, MAX_DEPTH as nat),
            Err(_) => true,
        },
{
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
            Fail { pos: q, msg: "unexpected trailing characters after the JSON value" }
        },
        Err(f) => f,
    };
    let (line, col) = location(src, f.pos);
    Err(ParseError { line, col, msg: f.msg })
}

} // verus!
