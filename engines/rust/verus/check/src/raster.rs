//! Replays the Zig engine's canvas calls through the verified raster kernels. The traces come from
//! a scratch build of the Zig CLI whose raster.zig appends, after the PNG, every `Canvas.init`,
//! `strokeFlat` (flattened points, half-width, dash), `fillEdges` (edges) and `flush` it made, then
//! the final gray8 pixels. Replaying the same calls must give the same pixels, byte for byte.

use kerf_core::raster::kernels::{Canvas, Edge, V};

struct Rd<'a>(&'a [u8], usize);

impl Rd<'_> {
    fn u8(&mut self) -> u8 {
        self.1 += 1;
        self.0[self.1 - 1]
    }
    fn u32(&mut self) -> usize {
        self.1 += 4;
        u32::from_le_bytes(self.0[self.1 - 4..self.1].try_into().unwrap()) as usize
    }
    fn f64(&mut self) -> f64 {
        self.1 += 8;
        f64::from_bits(u64::from_le_bytes(self.0[self.1 - 8..self.1].try_into().unwrap()))
    }
}

pub fn replay(trace: &[u8]) -> (Vec<u8>, Vec<u8>, usize, usize) {
    let mut r = Rd(trace, 0);
    let mut canvas: Option<Canvas> = None;
    let (mut strokes, mut fills) = (0, 0);
    loop {
        match r.u8() {
            b'N' => {
                let (w, h) = (r.u32(), r.u32());
                canvas = Some(Canvas::new(w, h));
            }
            b'S' => {
                let half = r.f64();
                let dash: Option<Vec<f64>> = (r.u8() == 1).then(|| (0..r.u32()).map(|_| r.f64()).collect());
                let pts: Vec<V> = (0..r.u32()).map(|_| V { x: r.f64(), y: r.f64() }).collect();
                canvas.as_mut().unwrap().stroke(&pts, half, dash.as_deref());
                strokes += 1;
            }
            b'F' => {
                let es: Vec<Edge> =
                    (0..r.u32()).map(|_| Edge { x0: r.f64(), y0: r.f64(), x1: r.f64(), y1: r.f64() }).collect();
                canvas.as_mut().unwrap().fill(&es);
                fills += 1;
            }
            b'U' => canvas.as_mut().unwrap().flush(),
            b'P' => {
                let want = trace[r.1..].to_vec();
                return (canvas.unwrap().into_pixels(), want, strokes, fills);
            }
            c => panic!("bad trace record {c}"),
        }
    }
}

pub fn check(paths: &[String]) -> bool {
    let mut ok = true;
    for p in paths {
        let bytes = std::fs::read(p).unwrap();
        let at = bytes.windows(8).position(|w| w == b"KVTRACE1").expect("trace marker");
        let (got, want, strokes, fills) = replay(&bytes[at + 8..]);
        let same = got == want;
        ok &= same;
        let diff = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        println!(
            "raster  {:>8} px  {strokes:>5} strokes {fills:>4} fills  {}  {p}",
            want.len(),
            if same { "identical".to_string() } else { format!("{diff} pixels DIFFER") }
        );
    }
    ok
}
