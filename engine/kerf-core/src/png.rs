mod deflate;

use alloc::vec::Vec;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const IHDR_FORMAT: [u8; 5] = [8, 0, 0, 0, 0];
const IDAT_MAX: usize = 65536;
const MAX_SIDE: u32 = 0x100_0000;
const ADLER_NMAX: usize = 5552;
const ADLER_MOD: u32 = 65521;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncodeError {
    InvalidDimensions,
    BadBufferSize,
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = (c >> 1) ^ if c & 1 == 1 { 0xEDB8_8320 } else { 0 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

fn crc_feed(crc: u32, bytes: &[u8]) -> u32 {
    bytes.iter().fold(crc, |r, &b| CRC_TABLE[usize::from((r as u8) ^ b)] ^ (r >> 8))
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for run in bytes.chunks(ADLER_NMAX) {
        for &x in run {
            a += u32::from(x);
            b += a;
        }
        a %= ADLER_MOD;
        b %= ADLER_MOD;
    }
    (b << 16) | a
}

fn zlib_frame(deflated: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(deflated.len() + 6);
    out.extend_from_slice(&[0x78, 0x9C]);
    out.extend_from_slice(deflated);
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn push_chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let crc = crc_feed(crc_feed(!0, tag), data);
    out.extend_from_slice(&(!crc).to_be_bytes());
}

fn container(width: u32, height: u32, zlib: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(zlib.len() + 64);
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&IHDR_FORMAT);
    push_chunk(&mut out, b"IHDR", &ihdr);
    for part in zlib.chunks(IDAT_MAX) {
        push_chunk(&mut out, b"IDAT", part);
    }
    push_chunk(&mut out, b"IEND", &[]);
    out
}

trait Predict {
    fn at(left: u8, up: u8, up_left: u8) -> u8;
}

struct NoFilter;
struct Sub;
struct Up;
struct Average;
struct Paeth;

impl Predict for NoFilter {
    #[inline(always)]
    fn at(_: u8, _: u8, _: u8) -> u8 {
        0
    }
}
impl Predict for Sub {
    #[inline(always)]
    fn at(left: u8, _: u8, _: u8) -> u8 {
        left
    }
}
impl Predict for Up {
    #[inline(always)]
    fn at(_: u8, up: u8, _: u8) -> u8 {
        up
    }
}
impl Predict for Average {
    #[inline(always)]
    fn at(left: u8, up: u8, _: u8) -> u8 {
        ((u16::from(left) + u16::from(up)) / 2) as u8
    }
}
impl Predict for Paeth {
    #[inline(always)]
    fn at(left: u8, up: u8, up_left: u8) -> u8 {
        let (a, b, c) = (i32::from(left), i32::from(up), i32::from(up_left));
        let p = a + b - c;
        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
        if pa <= pb && pa <= pc {
            left
        } else if pb <= pc {
            up
        } else {
            up_left
        }
    }
}

#[inline(always)]
fn magnitude(residual: u8) -> u64 {
    u64::from(if residual < 128 { residual } else { residual.wrapping_neg() })
}

#[inline(always)]
fn for_each_residual<P: Predict>(row: &[u8], prev: &[u8], mut sink: impl FnMut(usize, u8)) {
    let n = row.len();
    let (row, prev) = (&row[..n], &prev[..n]);
    if n == 0 {
        return;
    }
    sink(0, row[0].wrapping_sub(P::at(0, prev[0], 0)));
    for i in 1..n {
        sink(i, row[i].wrapping_sub(P::at(row[i - 1], prev[i], prev[i - 1])));
    }
}

fn score<P: Predict>(row: &[u8], prev: &[u8]) -> u64 {
    let mut total = 0;
    for_each_residual::<P>(row, prev, |_, r| total += magnitude(r));
    total
}

fn write<P: Predict>(row: &[u8], prev: &[u8], out: &mut [u8]) {
    let out = &mut out[..row.len()];
    for_each_residual::<P>(row, prev, |i, r| out[i] = r);
}

const FILTER_COUNT: u8 = 5;

fn filter_score(kind: u8, row: &[u8], prev: &[u8]) -> u64 {
    match kind {
        0 => score::<NoFilter>(row, prev),
        1 => score::<Sub>(row, prev),
        2 => score::<Up>(row, prev),
        3 => score::<Average>(row, prev),
        _ => score::<Paeth>(row, prev),
    }
}

fn filter_write(kind: u8, row: &[u8], prev: &[u8], out: &mut [u8]) {
    match kind {
        0 => write::<NoFilter>(row, prev, out),
        1 => write::<Sub>(row, prev, out),
        2 => write::<Up>(row, prev, out),
        3 => write::<Average>(row, prev, out),
        _ => write::<Paeth>(row, prev, out),
    }
}

fn filter_image(width: usize, pixels: &[u8]) -> Vec<u8> {
    let zero_row = alloc::vec![0u8; width];
    let mut out = Vec::with_capacity(pixels.len() + pixels.len() / width);
    let mut prev = zero_row.as_slice();
    for row in pixels.chunks_exact(width) {
        let mut best = (0, u64::MAX);
        for kind in 0..FILTER_COUNT {
            let s = filter_score(kind, row, prev);
            if s < best.1 {
                best = (kind, s);
            }
        }
        out.push(best.0);
        let start = out.len();
        out.resize(start + width, 0);
        filter_write(best.0, row, prev, &mut out[start..]);
        prev = row;
    }
    out
}

pub fn encode_gray(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(EncodeError::InvalidDimensions);
    }
    if u64::try_from(pixels.len()) != Ok(u64::from(width) * u64::from(height)) {
        return Err(EncodeError::BadBufferSize);
    }
    let filtered = filter_image(width as usize, pixels);
    let zlib = zlib_frame(&deflate::deflate_raw(&filtered), &filtered);
    Ok(container(width, height, &zlib))
}
