//! Designer image attachments: decode, downscale to <= 1568 px on the long side, re-encode PNG.

use crate::chat::Thumb;
use std::sync::Arc;

pub const MAX_SIDE: u32 = 1568;

pub fn prepare_attachment(name: &str, bytes: &[u8]) -> Result<Thumb, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a PNG/JPEG image ({e})"))?;
    let (w, h) = (img.width(), img.height());
    let img = if w.max(h) > MAX_SIDE {
        let k = MAX_SIDE as f32 / w.max(h) as f32;
        img.resize(((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = Vec::new();
    img.to_rgba8()
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| format!("png encode: {e}"))?;
    Ok(Thumb { name: name.to_owned(), media_type: "image/png".into(), png_or_jpg: Arc::new(out) })
}
