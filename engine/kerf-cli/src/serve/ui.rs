//! Static files of the embedded web UI and the Content-Security-Policy that goes with them.

use super::Server;
use crate::http::{self, Head, Out, Request};
use std::io;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));
}

pub fn has_ui() -> bool {
    !assets::FILES.is_empty()
}

pub fn static_file(s: &Server, req: &Request, w: &mut Out, path: &str) -> io::Result<bool> {
    let ka = req.keep_alive;
    let head_only = req.method == "HEAD";
    let mut rel = path.trim_start_matches('/');
    if rel.is_empty() {
        rel = "index.html";
    }
    if rel.contains("..") || rel.contains('\\') {
        http::send_error(w, 400, ka, "", "E_PATH", "bad path")?;
        return Ok(ka);
    }
    if let Some((p, data)) = assets::FILES.iter().find(|f| f.0 == rel) {
        let h = Head {
            status: 200,
            content_type: Some(http::mime_for(p)),
            keep_alive: ka,
            no_store: false,
            extra: "Cache-Control: no-cache\r\n",
            csp: Some(&s.ui_csp),
            ..Head::default()
        };
        if head_only {
            http::send_head_only(w, h, data.len())?
        } else {
            http::send(w, h, data)?
        }
        return Ok(ka);
    }
    if assets::FILES.is_empty() && rel == "index.html" {
        let h = Head { status: 200, content_type: Some("text/html; charset=utf-8"), keep_alive: ka, ..Head::default() };
        if head_only {
            http::send_head_only(w, h, NO_UI_PAGE.len())?
        } else {
            http::send(w, h, NO_UI_PAGE.as_bytes())?
        }
        return Ok(ka);
    }
    http::send_error(w, 404, ka, "", "E_NOT_FOUND", "not found")?;
    Ok(ka)
}

const CSP_PREFIX: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'";
const CSP_SUFFIX: &str = "; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self' data: blob:; worker-src 'self' blob:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

pub fn default_ui_csp() -> String {
    format!("{CSP_PREFIX}{CSP_SUFFIX}")
}

/// The UI's CSP plus a `'sha256-…'` source for every inline `<script>` of the embedded index.html.
pub fn build_ui_csp() -> String {
    let mut out = String::from(CSP_PREFIX);
    for (_, data) in assets::FILES.iter().filter(|f| f.0 == "index.html") {
        let html = String::from_utf8_lossy(data);
        let mut rest: &str = &html;
        while let Some(i) = rest.find("<script") {
            rest = &rest[i + "<script".len()..];
            let Some(tag_end) = rest.find('>') else { break };
            let attrs = &rest[..tag_end];
            let Some(close) = rest[tag_end + 1..].find("</script>") else { break };
            let body = &rest[tag_end + 1..][..close];
            rest = &rest[tag_end + 1 + close..];
            if attrs.contains("src=") || body.is_empty() {
                continue;
            }
            out.push_str(&format!(" 'sha256-{}'", base64(&sha256(body.as_bytes()))));
        }
    }
    out.push_str(CSP_SUFFIX);
    out
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in b.chunks(3) {
        let n = c.iter().enumerate().fold(0u32, |a, (i, &x)| a | u32::from(x) << (16 - 8 * i));
        for k in 0..=c.len() {
            s.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
        }
        for _ in c.len()..3 {
            s.push('=');
        }
    }
    s
}

fn sha256(msg: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut data = msg.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&(msg.len() as u64 * 8).to_be_bytes());
    for block in data.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (word, c) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*c);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    let mut out = [0u8; 32];
    for (c, x) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *c = x.to_be_bytes();
    }
    out
}

const NO_UI_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">
<title>kerf serve</title>
<style>body{font:15px/1.5 ui-monospace,Menlo,Consolas,monospace;max-width:46rem;margin:3rem auto;padding:0 1rem;background:#f4efe3;color:#1d2a3a}
code,pre{background:#e9e2d0;padding:.1rem .3rem}pre{padding:.7rem;overflow:auto}h1{font-size:1.2rem;border-bottom:2px solid #1d2a3a}</style></head><body>
<h1>KERF SERVE</h1>
<p>The server is running, but this binary was built <b>without the web UI</b>.</p>
<p>Build the UI, then rebuild the CLI with it embedded:</p>
<pre>cd apps/web &amp;&amp; npm ci &amp;&amp; npm run build:serve
cd ../../engine &amp;&amp; KERF_UI=../apps/web/dist-serve cargo build --release</pre>
<p>Release binaries from GitHub already include the UI. The API works regardless:
<a href=\"/api/info\">/api/info</a> &middot; <a href=\"/api/docs\">/api/docs</a></p>
</body></html>";
