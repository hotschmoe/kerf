//! PNG encoder: 8-bit gray / RGB / RGBA, adaptive scanline filters chosen per row by the minimum-sum-of-absolute-values
//! heuristic, DEFLATE through `deflate` (exact port of Zig's std.compress.flate), zlib framing, CRC-32 per chunk.
//! Deterministic.

#[path = "png/deflate.rs"]
mod deflate;

use alloc::vec::Vec;

pub fn crc_table() -> Vec<u32> {
    let mut t: Vec<u32> = Vec::with_capacity(256);
    let mut n: u32 = 0;
    while n < 256 {
        let mut c = n;
        let mut k: u32 = 0;
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
pub fn crc_feed(table: &[u32], c: u32, s: &[u8]) -> u32 {
    let mut r = c;
    let mut i: usize = 0;
    while i < s.len() {
        r = table[((r ^ s[i] as u32) & 0xff) as usize] ^ (r >> 8);
        i += 1;
    }
    r
}

/// Adler-32 with the modulo deferred to every 5552 bytes (zlib's NMAX: the largest run after which
/// `b` still fits in 32 bits), as Zig's std and zlib compute it.
pub fn adler32_of(s: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    let mut i: usize = 0;
    while i < s.len() {
        let end = if s.len() - i < 5552 {
            s.len()
        } else {
            i + 5552
        };
        while i < end {
            let x = s[i] as u32;
            a = a + x;
            b = b + a;
            i += 1;
        }
        a = a % 65521;
        b = b % 65521;
    }
    (b << 16) | a
}

/// Header, the raw deflate stream, and the Adler-32 of the uncompressed data, big-endian.
pub fn zlib_frame(deflate: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    push_bytes(&mut out, &[0x78u8, 0x9C]);
    push_bytes(&mut out, deflate);
    let a = adler32_of(raw);
    push_be32(&mut out, a);
    out
}

#[inline(always)]
fn push_be32(out: &mut Vec<u8>, x: u32) {
    out.push((x >> 24) as u8);
    out.push(((x >> 16) & 0xff) as u8);
    out.push(((x >> 8) & 0xff) as u8);
    out.push((x & 0xff) as u8);
}

fn push_chunk(out: &mut Vec<u8>, table: &[u32], tag: &[u8], data: &[u8]) {
    push_be32(out, data.len() as u32);
    push_bytes(out, tag);
    push_bytes(out, data);
    let c = crc_feed(table, 0xFFFF_FFFF, tag);
    let c = crc_feed(table, c, data);
    push_be32(out, !c);
}

/// The PNG container around an already compressed zlib stream `z` of `channels`-byte pixels.
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
    push_chunk(
        &mut out,
        table.as_slice(),
        &[0x49u8, 0x48, 0x44, 0x52],
        h.as_slice(),
    );
    let mut off: usize = 0;
    while off < z.len() {
        let n = if z.len() - off < 65536 {
            z.len() - off
        } else {
            65536
        };
        push_chunk(
            &mut out,
            table.as_slice(),
            &[0x49u8, 0x44, 0x41, 0x54],
            slice_range(z, off, off + n),
        );
        off += n;
    }
    push_chunk(&mut out, table.as_slice(), &[0x49u8, 0x45, 0x4E, 0x44], &[]);
    out
}

fn slice_range(s: &[u8], a: usize, b: usize) -> &[u8] {
    &s[a..b]
}

#[inline(always)]
fn sub8(x: u8, p: u8) -> u8 {
    let f = x.wrapping_sub(p);
    f
}

#[inline(always)]
fn mag8(f: u8) -> u64 {
    let m: u8 = if f < 128 {
        f
    } else {
        let g = 0u8.wrapping_sub(f);
        g
    };
    m as u64
}

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

/// Filters `row` into `out` (one row): one pass specialised per filter type, as `filter_score`.
fn filter_row(kind: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
    let n = out.len();
    let rv = slice_range(row, 0, n);
    let pv = slice_range(prev, 0, n);
    let h = if bpp < n { bpp } else { n };
    let mut i: usize = 0;
    if kind == 0 {
        while i < n {
            out[i] = sub8(rv[i], 0);
            i += 1;
        }
    } else if kind == 1 {
        while i < h {
            out[i] = sub8(rv[i], 0);
            i += 1;
        }
        while i < n {
            out[i] = sub8(rv[i], rv[i - bpp]);
            i += 1;
        }
    } else if kind == 2 {
        while i < n {
            out[i] = sub8(rv[i], pv[i]);
            i += 1;
        }
    } else if kind == 3 {
        while i < h {
            out[i] = sub8(rv[i], pv[i] / 2);
            i += 1;
        }
        while i < n {
            out[i] = sub8(rv[i], ((rv[i - bpp] as u16 + pv[i] as u16) / 2) as u8);
            i += 1;
        }
    } else {
        while i < h {
            out[i] = sub8(rv[i], pv[i]);
            i += 1;
        }
        while i < n {
            out[i] = sub8(rv[i], paeth(rv[i - bpp], pv[i], pv[i - bpp]));
            i += 1;
        }
    }
}

/// `row_score(kind, ..)` without writing the filtered row: one pass specialised per filter type, split where `i`
/// reaches `bpp` (before it, the left and upper-left neighbours are 0).
fn filter_score(kind: u8, row: &[u8], prev: &[u8], bpp: usize) -> u64 {
    let n = row.len();
    let pv = slice_range(prev, 0, n);
    let h = if bpp < n { bpp } else { n };
    let mut s: u64 = 0;
    let mut i: usize = 0;
    if kind == 0 {
        while i < n {
            s = s + mag8(row[i]);
            i += 1;
        }
    } else if kind == 1 {
        while i < h {
            s = s + mag8(sub8(row[i], 0));
            i += 1;
        }
        while i < n {
            s = s + mag8(sub8(row[i], row[i - bpp]));
            i += 1;
        }
    } else if kind == 2 {
        while i < n {
            s = s + mag8(sub8(row[i], pv[i]));
            i += 1;
        }
    } else if kind == 3 {
        while i < h {
            s = s + mag8(sub8(row[i], pv[i] / 2));
            i += 1;
        }
        while i < n {
            s = s + mag8(sub8(row[i], ((row[i - bpp] as u16 + pv[i] as u16) / 2) as u8));
            i += 1;
        }
    } else {
        while i < h {
            s = s + mag8(sub8(row[i], pv[i]));
            i += 1;
        }
        while i < n {
            s = s + mag8(sub8(row[i], paeth(row[i - bpp], pv[i], pv[i - bpp])));
            i += 1;
        }
    }
    s
}

/// The filtered image: per row, the filter byte with the lowest score (the earliest on ties)
/// followed by the row filtered with it. `None` for a zero or oversized dimension or a buffer of
/// the wrong length.
pub fn filter_image(width: u32, height: u32, channels: usize, pixels: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > 0x100_0000 || height > 0x100_0000 {
        return None;
    }
    if !(channels == 1 || channels == 3 || channels == 4) {
        return None;
    }
    let stride: usize = width as usize * channels;
    let plen = pixels.len();
    let total = stride as u64 * height as u64;
    if total != plen as u64 {
        return None;
    }
    let n64 = (stride as u64 + 1) * height as u64;
    if n64 > usize::MAX as u64 {
        return None;
    }
    let n = n64 as usize;
    let zero_row: Vec<u8> = vec_of(0u8, stride);
    let mut cand: Vec<u8> = vec_of(0u8, stride);
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut y: usize = 0;
    while y < height as usize {
        let row = slice_range(pixels, y * stride, y * stride + stride);
        let prev = if y == 0 {
            zero_row.as_slice()
        } else {
            slice_range(pixels, (y - 1) * stride, y * stride)
        };
        let mut best_kind: u8 = 0;
        let mut best_score: u64 = u64::MAX;
        let mut kind: u8 = 0;
        while kind < 5 {
            let sc = filter_score(kind, row, prev, channels);
            if sc < best_score {
                best_score = sc;
                best_kind = kind;
            }
            kind += 1;
        }
        filter_row(best_kind, row, prev, channels, cand.as_mut_slice());
        out.push(best_kind);
        push_bytes(&mut out, cand.as_slice());
        y += 1;
    }
    Some(out)
}

fn vec_of(x: u8, n: usize) -> Vec<u8> {
    let v = alloc::vec![x; n];
    v
}

#[inline(always)]
fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorType {
    Gray,
    Rgb,
    Rgba,
}

impl ColorType {
    pub fn channels(self) -> usize {
        match self {
            ColorType::Gray => 1,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncodeError {
    InvalidDimensions,
    BadBufferSize,
}

/// The DEFLATE compressor: an exact port of Zig's `std.compress.flate` at level default.
fn deflate_raw(data: &[u8]) -> Vec<u8> {
    deflate::deflate_raw(data)
}

/// Encodes an image. `pixels.len()` must equal `width * height * channels`.
pub fn encode(width: u32, height: u32, color: ColorType, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 || width > 0x100_0000 || height > 0x100_0000 {
        return Err(EncodeError::InvalidDimensions);
    }
    let ch = color.channels();
    let total = width as u64 * ch as u64 * height as u64;
    if total > usize::MAX as u64 {
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
