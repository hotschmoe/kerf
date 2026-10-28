//! Rebuilds each golden PDF with the verified writer, following pdf.zig's object sequence, from the
//! golden's own content stream and MediaBox numbers, and requires byte equality.

use kerf_core::pdf::writer::Writer;

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    h[from..].windows(n.len()).position(|w| w == n).map(|p| p + from)
}

fn rebuild(golden: &[u8]) -> Option<Vec<u8>> {
    let mb = find(golden, b"/MediaBox [0 0 ", 0)? + 15;
    let mb_end = find(golden, b"]", mb)?;
    let dims = &golden[mb..mb_end];
    let sp = dims.iter().position(|&c| c == b' ')?;
    let (w_txt, h_txt) = (&dims[..sp], &dims[sp + 1..]);
    let cs = find(golden, b"stream\n", 0)? + 7;
    let ce = find(golden, b"endstream\nendobj\n5 0 obj", cs)?;
    let content = &golden[cs..ce];

    let mut w = Writer::new();
    w.object(b"<< /Type /Catalog /Pages 2 0 R >>");
    w.object(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    let mut page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ".to_vec();
    page.extend_from_slice(w_txt);
    page.push(b' ');
    page.extend_from_slice(h_txt);
    page.extend_from_slice(b"] /Contents 4 0 R /Resources << >> >>");
    w.object(&page);
    w.stream_object(content);
    w.object(b"<< /Title (Kerf detail) /Producer (kerf-zig) >>");
    Some(w.finish(1, 5))
}

pub fn check(paths: &[String]) -> bool {
    let mut ok = true;
    for p in paths {
        let golden = std::fs::read(p).expect("read golden pdf");
        match rebuild(&golden) {
            Some(out) if out == golden => println!("pdf  identical  {:>8} bytes  {p}", out.len()),
            Some(out) => {
                let at = out.iter().zip(&golden).position(|(a, b)| a != b).unwrap_or(out.len().min(golden.len()));
                println!("pdf  DIFFERS at byte {at} ({} vs {} bytes)  {p}", out.len(), golden.len());
                ok = false;
            }
            None => {
                println!("pdf  unparseable golden  {p}");
                ok = false;
            }
        }
    }
    ok
}
