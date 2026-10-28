//! Minimal PNG encoder: 8-bit gray / RGB / RGBA, adaptive scanline filters chosen per row by the
//! minimum-sum-of-absolute-values heuristic, zlib/DEFLATE through `deflate`, CRC-32 per chunk.
//! Deterministic: fixed filter heuristic and fixed deflate level. Depends on nothing else in the engine.

mod deflate;

use alloc::vec;
use alloc::vec::Vec;

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

    fn code(self) -> u8 {
        match self {
            ColorType::Gray => 0,
            ColorType::Rgb => 2,
            ColorType::Rgba => 6,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncodeError {
    InvalidDimensions,
    BadBufferSize,
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = crc_table();

pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c = CRC_TABLE[usize::from((c as u8) ^ b)] ^ (c >> 8);
    }
    !c
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i32::from(a), i32::from(b), i32::from(c));
    let p = ia + ib - ic;
    let pa = (p - ia).abs();
    let pb = (p - ib).abs();
    let pc = (p - ic).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn filter_row(kind: u8, row: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) {
    for (i, (&x, o)) in row.iter().zip(out.iter_mut()).enumerate() {
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        *o = match kind {
            0 => x,
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            3 => x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
            _ => x.wrapping_sub(paeth(a, b, c)),
        };
    }
}

fn row_score(buf: &[u8]) -> u64 {
    buf.iter().map(|&v| u64::from((v as i8).unsigned_abs())).sum()
}

fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let crc = crc32_update(crc32_update(0, tag), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes an image. `pixels.len()` must equal `width * height * channels`.
pub fn encode(width: u32, height: u32, color: ColorType, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 || width > 1 << 24 || height > 1 << 24 {
        return Err(EncodeError::InvalidDimensions);
    }
    let ch = color.channels();
    let stride = (width as usize).checked_mul(ch).ok_or(EncodeError::InvalidDimensions)?;
    let total = stride.checked_mul(height as usize).ok_or(EncodeError::InvalidDimensions)?;
    if pixels.len() != total {
        return Err(EncodeError::BadBufferSize);
    }

    let mut filtered = vec![0u8; (stride + 1) * height as usize];
    let zero_row = vec![0u8; stride];
    let mut cand = vec![0u8; stride];
    for (y, (row, dst)) in pixels.chunks_exact(stride).zip(filtered.chunks_exact_mut(stride + 1)).enumerate() {
        let prev = if y == 0 { &zero_row[..] } else { &pixels[(y - 1) * stride..y * stride] };
        let mut best_kind = 0u8;
        let mut best_score = u64::MAX;
        for kind in 0..5u8 {
            filter_row(kind, row, prev, ch, &mut cand);
            let sc = row_score(&cand);
            if sc < best_score {
                best_score = sc;
                best_kind = kind;
            }
        }
        dst[0] = best_kind;
        filter_row(best_kind, row, prev, ch, &mut dst[1..]);
    }

    let z = deflate::zlib_compress(&filtered);

    let mut out = Vec::with_capacity(z.len() + 128);
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = color.code();
    write_chunk(&mut out, b"IHDR", &ihdr);
    for part in z.chunks(65536) {
        write_chunk(&mut out, b"IDAT", part);
    }
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}
