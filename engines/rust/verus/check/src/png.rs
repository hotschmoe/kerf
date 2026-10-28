//! Reproduces the Zig engine's golden raster PNGs with the verified PNG kernels. The deflate stream
//! is not ours (the engine uses Zig's std.compress.flate), so for each golden: inflate its IDAT
//! stream with png_inflate.zig, unfilter it to pixels, then require
//!   filter_image(pixels) == the inflated scanlines (filter choice and bytes),
//!   adler32_of(scanlines) == the zlib trailer, and zlib_frame rebuilds the zlib stream,
//!   png_container(width, height, channels, zlib stream) == the golden file, byte for byte.

use kerf_core::png::{adler32_of, filter_image, png_container, zlib_frame};

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let (pa, pb, pc) = ((p - a as i32).abs(), (p - b as i32).abs(), (p - c as i32).abs());
    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
}

fn unfilter(raw: &[u8], w: usize, h: usize, ch: usize) -> Vec<u8> {
    let stride = w * ch;
    let mut px = vec![0u8; stride * h];
    for y in 0..h {
        let kind = raw[y * (stride + 1)];
        for i in 0..stride {
            let x = raw[y * (stride + 1) + 1 + i];
            let a = if i >= ch { px[y * stride + i - ch] } else { 0 };
            let b = if y > 0 { px[(y - 1) * stride + i] } else { 0 };
            let c = if i >= ch && y > 0 { px[(y - 1) * stride + i - ch] } else { 0 };
            px[y * stride + i] = match kind {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                _ => x.wrapping_add(paeth(a, b, c)),
            };
        }
    }
    px
}

pub fn check(paths: &[String]) -> bool {
    let mut ok = true;
    for p in paths {
        let golden = std::fs::read(p).unwrap();
        let raw = std::fs::read(format!("{p}.raw")).expect("run png_inflate first");
        let (mut pos, mut w, mut h, mut ch) = (8usize, 0u32, 0u32, 0usize);
        let mut zlib = Vec::new();
        while pos + 12 <= golden.len() {
            let len = be32(&golden[pos..]) as usize;
            let tag = &golden[pos + 4..pos + 8];
            let data = &golden[pos + 8..pos + 8 + len];
            match tag {
                b"IHDR" => {
                    w = be32(data);
                    h = be32(&data[4..]);
                    ch = match data[9] { 0 => 1, 2 => 3, _ => 4 };
                }
                b"IDAT" => zlib.extend_from_slice(data),
                _ => {}
            }
            pos += 12 + len;
        }
        let pixels = unfilter(&raw, w as usize, h as usize, ch);
        let filtered = filter_image(w, h, ch, &pixels).expect("valid image");
        let adler = adler32_of(&raw);
        let trailer = be32(&zlib[zlib.len() - 4..]);
        let container = png_container(w, h, ch, &zlib);
        let framed = zlib_frame(&zlib[2..zlib.len() - 4], &raw);
        let (f, a, c) = (filtered == raw, adler == trailer && framed == zlib, container == golden);
        ok &= f && a && c;
        println!(
            "png  {w:>5}x{h:<5} ch{ch} zlib {:02x}{:02x}  filters {}  zlib frame {}  container {}  {p}",
            zlib[0],
            zlib[1],
            if f { "identical" } else { "DIFFER" },
            if a { "identical" } else { "DIFFERS" },
            if c { "identical" } else { "DIFFERS" },
        );
    }
    ok
}
