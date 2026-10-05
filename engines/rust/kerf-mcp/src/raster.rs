//! SVG -> PNG with resvg (pure Rust). White background, no text/raster-image support needed:
//! the engine draws its own stroke font as paths.

use resvg::{tiny_skia, usvg};

pub struct Png {
    pub bytes: Vec<u8>,
}

pub fn svg_to_png(svg: &str, width_px: u32) -> Result<Png, String> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("cannot parse engine SVG: {}", e))?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("engine SVG has an empty size".into());
    }
    let scale = width_px as f32 / size.width();
    let h = ((size.height() * scale).ceil() as u32).clamp(1, 4000);
    let mut pm = tiny_skia::Pixmap::new(width_px, h).ok_or("cannot allocate pixmap")?;
    pm.fill(tiny_skia::Color::WHITE);
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pm.as_mut());
    let bytes = pm.encode_png().map_err(|e| format!("png encode: {}", e))?;
    Ok(Png { bytes })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn b64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
