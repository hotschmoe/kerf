//! PNG encoder: 8-bit gray / RGB / RGBA, adaptive scanline filters chosen per row by the minimum-sum-of-absolute-values
//! heuristic, DEFLATE through `deflate` (unverified exact port of Zig's std.compress.flate), zlib framing, CRC-32 per chunk.
//! Proven with Verus (engines/rust/verus/VERUS.md): CRC-32 and Adler-32 equal their definitions, the zlib header and
//! trailer, the chunk/IHDR/IDAT/IEND framing and every filtered byte equal their specs. Deterministic.

#[path = "png/deflate.rs"]
mod deflate;

use alloc::vec::Vec;
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::div_mod::*;
#[cfg(verus_keep_ghost)]
use vstd::arithmetic::mul::*;
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg(verus_keep_ghost)]
verus! {

// ---- CRC-32 (ISO-HDLC, as PNG and zlib define it) ------------------------------------------------

/// One bit of the reflected CRC-32 division by 0xEDB88320.
pub open spec fn crc_step(c: u32) -> u32 {
    (c >> 1u32) ^ (if c & 1u32 == 1u32 { 0xEDB88320u32 } else { 0u32 })
}

pub open spec fn crc_steps(c: u32, k: nat) -> u32
    decreases k,
{
    if k == 0 {
        c
    } else {
        crc_steps(crc_step(c), (k - 1) as nat)
    }
}

/// The bitwise definition: xor the byte in, then shift out eight bits.
pub open spec fn crc_byte(c: u32, b: u8) -> u32 {
    crc_steps(c ^ (b as u32), 8)
}

pub open spec fn crc_update(c: u32, s: Seq<u8>) -> u32
    decreases s.len(),
{
    if s.len() == 0 {
        c
    } else {
        crc_byte(crc_update(c, s.drop_last()), s.last())
    }
}

pub open spec fn crc32(s: Seq<u8>) -> u32 {
    !crc_update(0xFFFF_FFFFu32, s)
}

proof fn lemma_step_linear(x: u32, y: u32)
    ensures
        crc_step(x ^ y) == crc_step(x) ^ crc_step(y),
{
    assert(((x ^ y) >> 1u32) ^ (if (x ^ y) & 1u32 == 1u32 { 0xEDB88320u32 } else { 0u32 }) == ((x
        >> 1u32) ^ (if x & 1u32 == 1u32 { 0xEDB88320u32 } else { 0u32 })) ^ ((y >> 1u32) ^ (if y & 1u32
        == 1u32 { 0xEDB88320u32 } else { 0u32 }))) by (bit_vector);
}

proof fn lemma_steps_linear(x: u32, y: u32, k: nat)
    ensures
        crc_steps(x ^ y, k) == crc_steps(x, k) ^ crc_steps(y, k),
    decreases k,
{
    if k > 0 {
        lemma_step_linear(x, y);
        lemma_steps_linear(crc_step(x), crc_step(y), (k - 1) as nat);
    }
}

proof fn lemma_step_even(c: u32)
    requires
        c & 1u32 == 0,
    ensures
        crc_step(c) == c >> 1u32,
{
    assert(((c >> 1u32) ^ (if c & 1u32 == 1u32 { 0xEDB88320u32 } else { 0u32 })) == c >> 1u32) by (bit_vector)
        requires
            c & 1u32 == 0,
    ;
}

proof fn lemma_steps_high(h: u32)
    requires
        h & 0xffu32 == 0,
    ensures
        crc_steps(h, 8) == h >> 8u32,
{
    let h1 = h >> 1u32;
    let h2 = h1 >> 1u32;
    let h3 = h2 >> 1u32;
    let h4 = h3 >> 1u32;
    let h5 = h4 >> 1u32;
    let h6 = h5 >> 1u32;
    let h7 = h6 >> 1u32;
    assert(h & 1u32 == 0 && h1 & 1u32 == 0 && h2 & 1u32 == 0 && h3 & 1u32 == 0 && h4 & 1u32 == 0 && h5
        & 1u32 == 0 && h6 & 1u32 == 0 && h7 & 1u32 == 0 && h7 >> 1u32 == h >> 8u32) by (bit_vector)
        requires
            h & 0xffu32 == 0,
            h1 == h >> 1u32,
            h2 == h1 >> 1u32,
            h3 == h2 >> 1u32,
            h4 == h3 >> 1u32,
            h5 == h4 >> 1u32,
            h6 == h5 >> 1u32,
            h7 == h6 >> 1u32,
    ;
    lemma_step_even(h);
    lemma_step_even(h1);
    lemma_step_even(h2);
    lemma_step_even(h3);
    lemma_step_even(h4);
    lemma_step_even(h5);
    lemma_step_even(h6);
    lemma_step_even(h7);
    reveal_with_fuel(crc_steps, 9);
}

/// The table-driven update computes the bitwise definition.
proof fn lemma_table_byte(c: u32, b: u8)
    ensures
        crc_byte(c, b) == crc_steps((c ^ (b as u32)) & 0xffu32, 8) ^ (c >> 8u32),
{
    let x = c ^ (b as u32);
    let lo = x & 0xffu32;
    let hi = x & 0xffff_ff00u32;
    assert(x == lo ^ hi && hi & 0xffu32 == 0 && hi >> 8u32 == c >> 8u32) by (bit_vector)
        requires
            x == c ^ (b as u32),
            lo == x & 0xffu32,
            hi == x & 0xffff_ff00u32,
            b <= 255,
    ;
    lemma_steps_linear(lo, hi, 8);
    lemma_steps_high(hi);
}

/// `table[n]` is the CRC of the single byte `n` without pre/post conditioning.
pub open spec fn crc_table_ok(t: Seq<u32>) -> bool {
    &&& t.len() == 256
    &&& forall|n: int| 0 <= n < 256 ==> #[trigger] t[n] == crc_steps(n as u32, 8)
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(t =>
    ensures
        crc_table_ok(t@),
))]
pub fn crc_table() -> Vec<u32> {
    let mut t: Vec<u32> = Vec::with_capacity(256);
    let mut n: u32 = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            n <= 256,
            t@.len() == n,
            forall|i: int| 0 <= i < n ==> #[trigger] t@[i] == crc_steps(i as u32, 8),
        decreases 256 - n,
    ))]
    while n < 256 {
        let mut c = n;
        let mut k: u32 = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                k <= 8,
                n < 256,
                crc_steps(n, 8) == crc_steps(c, (8 - k) as nat),
            decreases 8 - k,
        ))]
        while k < 8 {
            c = (c >> 1) ^ (if c & 1 == 1 { 0xEDB88320 } else { 0 });
            k += 1;
        }
        t.push(c);
        n += 1;
    }
    t
}

/// Feeds `s` into a running (pre-conditioned) CRC register.
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        crc_table_ok(table@),
    ensures
        r == crc_update(c, s@),
))]
pub fn crc_feed(table: &[u32], c: u32, s: &[u8]) -> u32 {
    let mut r = c;
    let mut i: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            crc_table_ok(table@),
            i <= s@.len(),
            r == crc_update(c, s@.subrange(0, i as int)),
        decreases s.len() - i,
    ))]
    while i < s.len() {
        #[cfg(verus_keep_ghost)]
        proof! {
            lemma_table_byte(r, s@[i as int]);
            assert(s@.subrange(0, i + 1).drop_last() =~= s@.subrange(0, i as int));
            let b = s@[i as int];
            assert(((r ^ (b as u32)) & 0xffu32) < 256) by (bit_vector);
        }
        r = table[((r ^ s[i] as u32) & 0xff) as usize] ^ (r >> 8);
        i += 1;
    }
    #[cfg(verus_keep_ghost)]
    proof! { assert(s@.subrange(0, i as int) =~= s@); }
    r
}

#[cfg(verus_keep_ghost)]
verus! {

// ---- Adler-32 (RFC 1950 section 8.2) ------------------------------------------------------------

pub open spec fn adler_mod() -> nat {
    65521
}

pub open spec fn byte_sum(s: Seq<u8>) -> nat
    decreases s.len(),
{
    if s.len() == 0 {
        0
    } else {
        byte_sum(s.drop_last()) + s.last() as nat
    }
}

/// `(1 + D1) + (1 + D1 + D2) + ... + (1 + D1 + ... + Dn)`
pub open spec fn prefix_sums(s: Seq<u8>) -> nat
    decreases s.len(),
{
    if s.len() == 0 {
        0
    } else {
        prefix_sums(s.drop_last()) + 1 + byte_sum(s)
    }
}

/// `A = 1 + D1 + ... + Dn (mod 65521)`, `B = the sum of the A's (mod 65521)`, `Adler-32 = B * 65536 + A`.
pub open spec fn adler32(s: Seq<u8>) -> nat {
    (prefix_sums(s) % adler_mod()) * 65536 + (1 + byte_sum(s)) % adler_mod()
}

proof fn lemma_adler_step(a: nat, b: nat, a0: nat, b0: nat, sum: nat, ps: nat, x: nat)
    requires
        a % 65521 == (1 + sum) % 65521,
        b % 65521 == ps % 65521,
        a0 == a + x,
        b0 == b + a0,
    ensures
        a0 % 65521 == (1 + (sum + x)) % 65521,
        b0 % 65521 == (ps + 1 + (sum + x)) % 65521,
{
    lemma_add_mod_noop(a as int, x as int, 65521);
    lemma_add_mod_noop((1 + sum) as int, x as int, 65521);
    lemma_add_mod_noop(b as int, a0 as int, 65521);
    lemma_add_mod_noop(ps as int, (1 + sum + x) as int, 65521);
}

} // verus!

/// Adler-32 with the modulo deferred to every 5552 bytes (zlib's NMAX: the largest run after which
/// `b` still fits in 32 bits), as Zig's std and zlib compute it.
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r as nat == adler32(s@),
))]
pub fn adler32_of(s: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    let mut i: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            i <= s@.len(),
            a as nat == (1 + byte_sum(s@.subrange(0, i as int))) % adler_mod(),
            b as nat == prefix_sums(s@.subrange(0, i as int)) % adler_mod(),
        decreases s.len() - i,
    ))]
    while i < s.len() {
        let end = if s.len() - i < 5552 {
            s.len()
        } else {
            i + 5552
        };
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost i0 = i; }
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                i0 <= i <= end <= s@.len(),
                end - i0 <= 5552,
                a % 65521 == (1 + byte_sum(s@.subrange(0, i as int))) % 65521,
                b % 65521 == prefix_sums(s@.subrange(0, i as int)) % 65521,
                a <= 65520 + 255 * (i - i0),
                2 * b <= 131040 * (i - i0 + 1) + 255 * (i - i0) * (i - i0 + 1),
            decreases end - i,
        ))]
        while i < end {
            #[cfg(verus_keep_ghost)]
            proof_decl! {
                let ghost p0 = s@.subrange(0, i as int);
                let ghost p1 = s@.subrange(0, i + 1);
                let ghost t = (i - i0) as int;
            }
            let x = s[i] as u32;
            #[cfg(verus_keep_ghost)]
            proof! {
                assert(p1.drop_last() =~= p0);
                assert(131040 * (t + 2) + 255 * (t + 1) * (t + 2) == 131040 * (t + 1) + 255 * t * (t + 1)
                    + 2 * (65520 + 255 * (t + 1))) by (nonlinear_arith);
                assert(131040 * (t + 2) + 255 * (t + 1) * (t + 2) <= 8_589_934_590) by (nonlinear_arith)
                    requires
                        0 <= t < 5552,
                ;
                lemma_adler_step(a as nat, b as nat, (a + x) as nat, (b + a + x) as nat, byte_sum(p0), prefix_sums(p0), x as nat);
            }
            a = a + x;
            b = b + a;
            i += 1;
        }
        a = a % 65521;
        b = b % 65521;
    }
    #[cfg(verus_keep_ghost)]
    proof! { assert(s@.subrange(0, i as int) =~= s@); }
    #[cfg(verus_keep_ghost)]
    proof! { assert((b << 16u32) | a == b * 65536 + a) by (bit_vector)
        requires
            a < 65521u32,
            b < 65521u32,
    ; }
    (b << 16) | a
}

#[cfg(verus_keep_ghost)]
verus! {

// ---- zlib container (RFC 1950) -------------------------------------------------------------------

/// CMF 0x78: deflate (CM 8) with a 32 KiB window (CINFO 7); FLG 0x9C: FLEVEL 2 (default), no
/// preset dictionary, FCHECK making the 16-bit header a multiple of 31.
pub open spec fn zlib_header() -> Seq<u8> {
    seq![0x78u8, 0x9C]
}

pub proof fn lemma_zlib_header()
    ensures
        zlib_header()[0] & 0x0f == 8,
        zlib_header()[0] >> 4 == 7,
        zlib_header()[1] & 0x20 == 0,
        (zlib_header()[0] as nat * 256 + zlib_header()[1] as nat) % 31 == 0,
{
    assert((0x78u8 & 0x0fu8) == 8u8 && (0x78u8 >> 4u8) == 7u8 && (0x9Cu8 & 0x20u8) == 0u8) by (bit_vector);
    assert((0x78 * 256 + 0x9C) % 31 == 0nat) by (compute);
}

} // verus!

/// Header, the raw deflate stream, and the Adler-32 of the uncompressed data, big-endian.
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r@ == zlib_header() + deflate@ + be32(adler32(raw@) as u32),
))]
pub fn zlib_frame(deflate: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    push_bytes(&mut out, &[0x78u8, 0x9C]);
    push_bytes(&mut out, deflate);
    let a = adler32_of(raw);
    push_be32(&mut out, a);
    #[cfg(verus_keep_ghost)]
    proof! { assert(out@ =~= zlib_header() + deflate@ + be32(adler32(raw@) as u32)); }
    out
}

#[cfg(verus_keep_ghost)]
verus! {

// ---- chunks and container (PNG 1.2 section 5) ----------------------------------------------------

pub open spec fn be32(x: u32) -> Seq<u8> {
    seq![(x >> 24u32) as u8, ((x >> 16u32) & 0xffu32) as u8, ((x >> 8u32) & 0xffu32) as u8, (x & 0xffu32) as u8]
}

/// Length, type, data, CRC of type and data.
pub open spec fn chunk(tag: Seq<u8>, data: Seq<u8>) -> Seq<u8> {
    be32(data.len() as u32) + tag + data + be32(crc32(tag + data))
}

pub open spec fn signature() -> Seq<u8> {
    seq![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
}

pub open spec fn tag_ihdr() -> Seq<u8> {
    seq![0x49u8, 0x48, 0x44, 0x52]
}

pub open spec fn tag_idat() -> Seq<u8> {
    seq![0x49u8, 0x44, 0x41, 0x54]
}

pub open spec fn tag_iend() -> Seq<u8> {
    seq![0x49u8, 0x45, 0x4E, 0x44]
}

pub open spec fn color_code(channels: nat) -> u8 {
    if channels == 1 {
        0
    } else if channels == 3 {
        2
    } else {
        6
    }
}

/// Width, height, bit depth 8, color type, deflate, filter method 0, no interlace.
pub open spec fn ihdr(width: u32, height: u32, channels: nat) -> Seq<u8> {
    be32(width) + be32(height) + seq![8u8, color_code(channels), 0, 0, 0]
}

/// The zlib stream as IDAT chunks of at most 64 KiB each.
pub open spec fn idat_chunks(z: Seq<u8>) -> Seq<u8>
    decreases z.len(),
{
    if z.len() == 0 {
        Seq::empty()
    } else if z.len() <= 65536 {
        chunk(tag_idat(), z)
    } else {
        chunk(tag_idat(), z.subrange(0, 65536)) + idat_chunks(z.subrange(65536, z.len() as int))
    }
}

pub open spec fn png_file(width: u32, height: u32, channels: nat, z: Seq<u8>) -> Seq<u8> {
    signature() + chunk(tag_ihdr(), ihdr(width, height, channels)) + idat_chunks(z) + chunk(
        tag_iend(),
        Seq::empty(),
    )
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + be32(x),
))]
#[inline(always)]
fn push_be32(out: &mut Vec<u8>, x: u32) {
    out.push((x >> 24) as u8);
    out.push(((x >> 16) & 0xff) as u8);
    out.push(((x >> 8) & 0xff) as u8);
    out.push((x & 0xff) as u8);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + be32(x)); }
}

#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        crc_table_ok(table@),
        data@.len() <= 0x7fff_ffff,
    ensures
        final(out)@ == old(out)@ + chunk(tag@, data@),
))]
fn push_chunk(out: &mut Vec<u8>, table: &[u32], tag: &[u8], data: &[u8]) {
    push_be32(out, data.len() as u32);
    push_bytes(out, tag);
    push_bytes(out, data);
    let c = crc_feed(table, 0xFFFF_FFFF, tag);
    let c = crc_feed(table, c, data);
    #[cfg(verus_keep_ghost)]
    proof! {
        lemma_crc_concat(0xFFFF_FFFFu32, tag@, data@);
    }
    push_be32(out, !c);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + chunk(tag@, data@)); }
}

#[cfg(verus_keep_ghost)]
verus! {

proof fn lemma_crc_concat(c: u32, s: Seq<u8>, t: Seq<u8>)
    ensures
        crc_update(c, s + t) == crc_update(crc_update(c, s), t),
    decreases t.len(),
{
    if t.len() > 0 {
        assert((s + t).drop_last() =~= s + t.drop_last());
        lemma_crc_concat(c, s, t.drop_last());
    } else {
        assert(s + t =~= s);
    }
}

} // verus!

/// The PNG container around an already compressed zlib stream `z` of `channels`-byte pixels.
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        channels == 1 || channels == 3 || channels == 4,
    ensures
        r@ == png_file(width, height, channels as nat, z@),
))]
pub fn png_container(width: u32, height: u32, channels: usize, z: &[u8]) -> Vec<u8> {
    let table = crc_table();
    let mut out: Vec<u8> = Vec::new();
    push_bytes(
        &mut out,
        &[0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    );
    let mut h: Vec<u8> = Vec::with_capacity(13);
    push_be32(&mut h, width);
    push_be32(&mut h, height);
    let code: u8 = if channels == 1 {
        0
    } else if channels == 3 {
        2
    } else {
        6
    };
    push_bytes(&mut h, &[8u8, code, 0, 0, 0]);
    #[cfg(verus_keep_ghost)]
    proof! { assert(h@ =~= ihdr(width, height, channels as nat)); }
    push_chunk(
        &mut out,
        table.as_slice(),
        &[0x49u8, 0x48, 0x44, 0x52],
        h.as_slice(),
    );
    #[cfg(verus_keep_ghost)]
    proof_decl! { let ghost head = out@; }
    let mut off: usize = 0;
    #[cfg(verus_keep_ghost)]
    proof! { assert(z@.subrange(0, z@.len() as int) =~= z@); }
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            crc_table_ok(table@),
            off <= z@.len(),
            out@ + idat_chunks(z@.subrange(off as int, z@.len() as int)) == head + idat_chunks(z@),
        decreases z.len() - off,
    ))]
    while off < z.len() {
        let n = if z.len() - off < 65536 {
            z.len() - off
        } else {
            65536
        };
        #[cfg(verus_keep_ghost)]
        proof_decl! {
            let ghost rest = z@.subrange(off as int, z@.len() as int);
            let ghost before = out@;
        }
        push_chunk(
            &mut out,
            table.as_slice(),
            &[0x49u8, 0x44, 0x41, 0x54],
            slice_range(z, off, off + n),
        );
        #[cfg(verus_keep_ghost)]
        proof! {
            if rest.len() > 65536 {
                assert(rest.subrange(0, 65536) =~= z@.subrange(off as int, off + n));
                assert(rest.subrange(65536, rest.len() as int) =~= z@.subrange(off + n, z@.len() as int));
            } else {
                assert(rest =~= z@.subrange(off as int, off + n));
                assert(z@.subrange(off + n, z@.len() as int) =~= Seq::<u8>::empty());
            }
            assert(out@ + idat_chunks(z@.subrange(off + n, z@.len() as int)) =~= before + idat_chunks(rest));
        }
        off += n;
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(z@.subrange(off as int, z@.len() as int) =~= Seq::<u8>::empty());
        assert(head + idat_chunks(z@) =~= out@);
    }
    push_chunk(&mut out, table.as_slice(), &[0x49u8, 0x45, 0x4E, 0x44], &[]);
    #[cfg(verus_keep_ghost)]
    proof! { assert(out@ =~= png_file(width, height, channels as nat, z@)); }
    out
}

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    requires
        a <= b <= s@.len(),
    ensures
        r@ == s@.subrange(a as int, b as int),
))]
fn slice_range(s: &[u8], a: usize, b: usize) -> &[u8] {
    &s[a..b]
}

#[cfg(verus_keep_ghost)]
verus! {

// ---- scanline filters (PNG 1.2 section 6) -------------------------------------------------------

pub open spec fn paeth_spec(a: u8, b: u8, c: u8) -> u8 {
    let p = a as int + b as int - c as int;
    let pa = if p >= a { p - a } else { a - p };
    let pb = if p >= b { p - b } else { b - p };
    let pc = if p >= c { p - c } else { c - p };
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filtered byte `i` of `row` (previous row `prev`, `bpp` bytes per pixel) under filter `kind`.
pub open spec fn filter_byte(kind: u8, row: Seq<u8>, prev: Seq<u8>, bpp: nat, i: int) -> u8 {
    let x = row[i];
    let a: u8 = if i >= bpp { row[i - bpp] } else { 0 };
    let b: u8 = prev[i];
    let c: u8 = if i >= bpp { prev[i - bpp] } else { 0 };
    let pred: u8 = if kind == 0 {
        0
    } else if kind == 1 {
        a
    } else if kind == 2 {
        b
    } else if kind == 3 {
        ((a as nat + b as nat) / 2) as u8
    } else {
        paeth_spec(a, b, c)
    };
    ((x as int - pred as int + 256) % 256) as u8
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r == paeth_spec(a, b, c),
))]
#[inline(always)]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p: i32 = a as i32 + b as i32 - c as i32;
    let pa = if p >= a as i32 {
        p - a as i32
    } else {
        a as i32 - p
    };
    let pb = if p >= b as i32 {
        p - b as i32
    } else {
        b as i32 - p
    };
    let pc = if p >= c as i32 {
        p - c as i32
    } else {
        c as i32 - p
    };
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filters `row` into `out` (one row) and returns the row's score (sum of the filtered bytes read
/// as signed magnitudes).
#[cfg_attr(verus_keep_ghost, verus_spec(score =>
    requires
        row@.len() == prev@.len() == old(out)@.len(),
        bpp >= 1,
        row@.len() <= 0x1_0000_0000,
    ensures
        final(out)@.len() == old(out)@.len(),
        forall|i: int| 0 <= i < row@.len() ==> #[trigger] final(out)@[i] == filter_byte(kind, row@, prev@, bpp as nat, i),
))]
fn filter_row(kind: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) -> u64 {
    let mut s: u64 = 0;
    let mut i: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            row@.len() == prev@.len() == out@.len(),
            bpp >= 1,
            i <= row@.len() <= 0x1_0000_0000,
            s <= 128 * i,
            forall|j: int| 0 <= j < i ==> #[trigger] out@[j] == filter_byte(kind, row@, prev@, bpp as nat, j),
        decreases row.len() - i,
    ))]
    while i < row.len() {
        let x = row[i];
        let a: u8 = if i >= bpp { row[i - bpp] } else { 0 };
        let b: u8 = prev[i];
        let c: u8 = if i >= bpp { prev[i - bpp] } else { 0 };
        let pred: u8 = if kind == 0 {
            0
        } else if kind == 1 {
            a
        } else if kind == 2 {
            b
        } else if kind == 3 {
            ((a as u16 + b as u16) / 2) as u8
        } else {
            paeth(a, b, c)
        };
        let f: u8 = x.wrapping_sub(pred);
        #[cfg(verus_keep_ghost)]
        proof! { assert(f == ((x as int - pred as int + 256) % 256) as u8) by {
            if x >= pred {
                lemma_mod_add_multiples_vanish((x - pred) as int, 256);
                lemma_small_mod((x - pred) as nat, 256);
            } else {
                lemma_small_mod((x as int - pred as int + 256) as nat, 256);
            }
        }; }
        out[i] = f;
        s = s + if f < 128 { f as u64 } else { 256 - f as u64 };
        i += 1;
    }
    s
}

/// The filtered image: per row, the filter byte with the lowest score (the earliest on ties)
/// followed by the row filtered with it. `None` for a zero or oversized dimension or a buffer of
/// the wrong length.
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r.is_none() ==> width == 0 || height == 0 || width > 0x100_0000 || height > 0x100_0000 || !(channels == 1
            || channels == 3 || channels == 4) || pixels@.len() != width * channels * height || (width * channels + 1)
            * height > usize::MAX,
        r.is_some() ==> {
            let stride = width * channels;
            &&& 1 <= width <= 0x100_0000
            &&& 1 <= height <= 0x100_0000
            &&& channels == 1 || channels == 3 || channels == 4
            &&& pixels@.len() == stride * height
            &&& r.unwrap()@.len() == (stride + 1) * height
            &&& forall|y: int, i: int|
                0 <= y < height && 0 <= i < stride ==> #[trigger] r.unwrap()@[y * (stride + 1) + 1 + i]
                    == filter_byte(
                    r.unwrap()@[y * (stride + 1)],
                    pixels@.subrange(y * stride, y * stride + stride),
                    if y == 0 {
                        Seq::new(stride as nat, |k: int| 0u8)
                    } else {
                        pixels@.subrange((y - 1) * stride, y * stride)
                    },
                    channels as nat,
                    i,
                )
            &&& forall|y: int| 0 <= y < height ==> #[trigger] r.unwrap()@[y * (stride + 1)] <= 4
        },
))]
pub fn filter_image(width: u32, height: u32, channels: usize, pixels: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > 0x100_0000 || height > 0x100_0000 {
        return None;
    }
    if !(channels == 1 || channels == 3 || channels == 4) {
        return None;
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(width as int * channels as int <= 0x400_0000) by (nonlinear_arith)
            requires
                width <= 0x100_0000,
                channels <= 4,
        ;
    }
    let stride: usize = width as usize * channels;
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(stride as int * height as int <= 0x4_0000_0000_0000 && (stride as int + 1) * height as int
            <= 0x4_0000_0100_0000) by (nonlinear_arith)
            requires
                stride <= 0x400_0000,
                height <= 0x100_0000,
        ;
    }
    let plen = pixels.len();
    let total = stride as u64 * height as u64;
    if total != plen as u64 {
        return None;
    }
    let n64 = (stride as u64 + 1) * height as u64;
    if n64 > usize::MAX as u64 {
        return None;
    }
    #[cfg(verus_keep_ghost)]
    proof_decl! {
        let ghost st = stride as int;
        let ghost hh = height as int;
    }
    let n = n64 as usize;
    let zero_row: Vec<u8> = vec_of(0u8, stride);
    let mut cand: Vec<u8> = vec_of(0u8, stride);
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut y: usize = 0;
    #[cfg_attr(verus_keep_ghost, verus_spec(
        invariant
            stride == st == width * channels,
            1 <= stride <= 0x400_0000,
            hh == height,
            1 <= height <= 0x100_0000,
            channels == 1 || channels == 3 || channels == 4,
            pixels@.len() == st * hh,
            plen == pixels@.len(),
            n == (st + 1) * hh,
            out@.len() == y * (st + 1),
            zero_row@ == Seq::new(stride as nat, |k: int| 0u8),
            cand@.len() == stride,
            y <= height,
            forall|yy: int, i: int|
                0 <= yy < y && 0 <= i < st ==> #[trigger] out@[yy * (st + 1) + 1 + i] == filter_byte(
                    out@[yy * (st + 1)],
                    pixels@.subrange(yy * st, yy * st + st),
                    if yy == 0 {
                        Seq::new(stride as nat, |k: int| 0u8)
                    } else {
                        pixels@.subrange((yy - 1) * st, yy * st)
                    },
                    channels as nat,
                    i,
                ),
            forall|yy: int| 0 <= yy < y ==> #[trigger] out@[yy * (st + 1)] <= 4,
        decreases height - y,
    ))]
    while y < height as usize {
        #[cfg(verus_keep_ghost)]
        proof! {
            assert((y + 1) * st <= st * hh && y * st + st == (y + 1) * st) by (nonlinear_arith)
                requires
                    y + 1 <= hh,
                    st >= 0,
            ;
            assert(y * (st + 1) + st + 1 == (y + 1) * (st + 1)) by (nonlinear_arith);
            if y > 0 {
                assert((y - 1) * st + st == y * st) by (nonlinear_arith);
            }
        }
        let row = slice_range(pixels, y * stride, y * stride + stride);
        let prev = if y == 0 {
            zero_row.as_slice()
        } else {
            slice_range(pixels, (y - 1) * stride, y * stride)
        };
        let mut best_kind: u8 = 0;
        let mut best_score: u64 = u64::MAX;
        let mut kind: u8 = 0;
        #[cfg_attr(verus_keep_ghost, verus_spec(
            invariant
                kind <= 5,
                best_kind <= 4,
                row@.len() == prev@.len() == stride <= 0x400_0000,
                cand@.len() == stride,
                channels >= 1,
            decreases 5 - kind,
        ))]
        while kind < 5 {
            let sc = filter_row(kind, row, prev, channels, cand.as_mut_slice());
            if sc < best_score {
                best_score = sc;
                best_kind = kind;
            }
            kind += 1;
        }
        filter_row(best_kind, row, prev, channels, cand.as_mut_slice());
        #[cfg(verus_keep_ghost)]
        proof_decl! { let ghost before = out@; }
        out.push(best_kind);
        push_bytes(&mut out, cand.as_slice());
        #[cfg(verus_keep_ghost)]
        proof! {
            let o = y * (st + 1);
            assert(out@ =~= before + seq![best_kind] + cand@);
            assert forall|yy: int, i: int|
                0 <= yy < y + 1 && 0 <= i < st implies #[trigger] out@[yy * (st + 1) + 1 + i]
                == filter_byte(
                out@[yy * (st + 1)],
                pixels@.subrange(yy * st, yy * st + st),
                if yy == 0 {
                    Seq::new(stride as nat, |k: int| 0u8)
                } else {
                    pixels@.subrange((yy - 1) * st, yy * st)
                },
                channels as nat,
                i,
            ) by {
                if yy < y {
                    lemma_mul_inequality(yy + 1, y as int, st + 1);
                    lemma_mul_is_distributive_add_other_way(st + 1, yy, 1);
                    assert(out@[yy * (st + 1)] == before[yy * (st + 1)]);
                    assert(out@[yy * (st + 1) + 1 + i] == before[yy * (st + 1) + 1 + i]);
                } else {
                    assert(out@[o] == best_kind);
                    assert(out@[o + 1 + i] == cand@[i]);
                }
            }
            assert forall|yy: int| 0 <= yy < y + 1 implies #[trigger] out@[yy * (st + 1)] <= 4 by {
                if yy < y {
                    lemma_mul_inequality(yy + 1, y as int, st + 1);
                    lemma_mul_is_distributive_add_other_way(st + 1, yy, 1);
                    assert(out@[yy * (st + 1)] == before[yy * (st + 1)]);
                }
            }
        }
        y += 1;
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(height as int * (st + 1) == (st + 1) * hh) by (nonlinear_arith)
            requires
                hh == height,
        ;
    }
    Some(out)
}

#[cfg_attr(verus_keep_ghost, verus_spec(v =>
    ensures
        v@ == Seq::new(n as nat, |k: int| x),
))]
fn vec_of(x: u8, n: usize) -> Vec<u8> {
    let v = alloc::vec![x; n];
    #[cfg(verus_keep_ghost)]
    proof! { assert(v@ =~= Seq::new(n as nat, |k: int| x)); }
    v
}

#[cfg_attr(verus_keep_ghost, verus_spec(
    ensures
        final(out)@ == old(out)@ + bytes@,
))]
#[inline(always)]
fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
    #[cfg(verus_keep_ghost)]
    proof! { assert(final(out)@ =~= old(out)@ + bytes@); }
}

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorType {
    Gray,
    Rgb,
    Rgba,
}

#[cfg(verus_keep_ghost)]
verus! {

pub open spec fn channels_of(c: ColorType) -> nat {
    match c {
        ColorType::Gray => 1,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
    }
}

} // verus!

#[cfg_attr(verus_keep_ghost, verus_verify)]
impl ColorType {
    #[cfg_attr(verus_keep_ghost, verus_spec(r =>
        ensures
            r == channels_of(self),
    ))]
    pub fn channels(self) -> usize {
        match self {
            ColorType::Gray => 1,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }
}

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncodeError {
    InvalidDimensions,
    BadBufferSize,
}

/// The DEFLATE compressor is trusted (not verified): an exact port of Zig's `std.compress.flate` at level default.
#[cfg_attr(verus_keep_ghost, verifier::external_body)]
fn deflate_raw(data: &[u8]) -> Vec<u8> {
    deflate::deflate_raw(data)
}

/// Encodes an image. `pixels.len()` must equal `width * height * channels`.
#[cfg_attr(verus_keep_ghost, verus_spec(r =>
    ensures
        r matches Ok(f) ==> exists|d: Seq<u8>, raw: Seq<u8>| f@ == png_file(
            width,
            height,
            channels_of(color),
            zlib_header() + d + be32(adler32(raw) as u32),
        ),
        r matches Err(EncodeError::InvalidDimensions) ==> width == 0 || height == 0 || width > 0x100_0000
            || height > 0x100_0000 || (width as int * channels_of(color) + 1) * height > usize::MAX,
))]
pub fn encode(width: u32, height: u32, color: ColorType, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 || width > 0x100_0000 || height > 0x100_0000 {
        return Err(EncodeError::InvalidDimensions);
    }
    let ch = color.channels();
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(width as int * ch as int <= 0x400_0000 && width as int * ch as int * height as int <= 0x4_0000_0000_0000)
            by (nonlinear_arith)
            requires
                width <= 0x100_0000,
                ch <= 4,
                height <= 0x100_0000,
        ;
    }
    let total = width as u64 * ch as u64 * height as u64;
    if total > usize::MAX as u64 {
        #[cfg(verus_keep_ghost)]
        proof! {
            assert((width as int * ch as int + 1) * height as int >= width as int * ch as int * height as int)
                by (nonlinear_arith);
        }
        return Err(EncodeError::InvalidDimensions);
    }
    if pixels.len() as u64 != total {
        return Err(EncodeError::BadBufferSize);
    }
    let filtered = match filter_image(width, height, ch, pixels) {
        Some(f) => f,
        None => return Err(EncodeError::InvalidDimensions),
    };
    let z = zlib_frame(deflate_raw(filtered.as_slice()).as_slice(), filtered.as_slice());
    Ok(png_container(width, height, ch, z.as_slice()))
}
