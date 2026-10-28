//! In-process benchmark of `kerf_core::api::call` over the case set of `bench/gen_cases.mjs`.
//!
//!   cargo run --profile release --example bench -- <cases dir> [--filter S] [--exclude S] [--min-ms N] [--min-reps N]
//!   cargo run --profile release --example bench -- <cases dir> --loop N [--filter S]   (profiling: every case N times, no report)
//!   RUSTFLAGS="--cfg kerf_bench_alloc" cargo run --release --example bench -- <cases dir> --count   (allocations per call)
//!   RUSTFLAGS="--cfg kerf_bench_alloc" cargo run --release --example bench -- <cases dir> --arena   (per-call arena timing)
//!
//! Prints one TSV row per case: label, fn, median us, min us, reps, output bytes, status.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// The system allocator with counters, so the report can show allocation churn per call (`--count`). With `--arena`
/// (a measurement, not a product feature) every allocation made inside a timed call comes from a bump arena that is
/// reset after the call and its output are dropped: the upper bound of what a Zig-style per-call arena would save.
struct Counting;
static ARENA_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ARENA_TOP: AtomicU64 = AtomicU64::new(0);
const ARENA_SIZE: usize = 1 << 30;
static ARENA_BASE: AtomicU64 = AtomicU64::new(0);

fn arena_alloc(l: Layout) -> *mut u8 {
    let base = ARENA_BASE.load(Relaxed) as usize;
    let top = ARENA_TOP.load(Relaxed) as usize;
    let start = (base + top).next_multiple_of(l.align()) - base;
    assert!(start + l.size() <= ARENA_SIZE, "bench arena exhausted");
    ARENA_TOP.store((start + l.size()) as u64, Relaxed);
    (base + start) as *mut u8
}

fn in_arena(p: *mut u8) -> bool {
    let base = ARENA_BASE.load(Relaxed) as usize;
    (p as usize).wrapping_sub(base) < ARENA_SIZE
}
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size() as u64, Relaxed);
        if ARENA_ON.load(Relaxed) {
            return arena_alloc(l);
        }
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if in_arena(p) {
            return;
        }
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(n.saturating_sub(l.size()) as u64, Relaxed);
        if ARENA_ON.load(Relaxed) || in_arena(p) {
            let base = ARENA_BASE.load(Relaxed) as usize;
            if in_arena(p) && p as usize + l.size() == base + ARENA_TOP.load(Relaxed) as usize {
                // the newest block grows in place
                let start = p as usize - base;
                assert!(start + n <= ARENA_SIZE, "bench arena exhausted");
                ARENA_TOP.store((start + n) as u64, Relaxed);
                return p;
            }
            // SAFETY: a fresh layout of the requested size; copy the smaller of the two sizes.
            unsafe {
                let q = if ARENA_ON.load(Relaxed) {
                    arena_alloc(Layout::from_size_align_unchecked(n, l.align()))
                } else {
                    System.alloc(Layout::from_size_align_unchecked(n, l.align()))
                };
                std::ptr::copy_nonoverlapping(p, q, l.size().min(n));
                if !in_arena(p) {
                    System.dealloc(p, l);
                }
                return q;
            }
        }
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.realloc(p, l, n) }
    }
}

// Only with RUSTFLAGS="--cfg kerf_bench_alloc": the counters cost an atomic add per allocation, which the timing runs
// must not pay (they use the plain system allocator, like the CLI).
#[cfg_attr(kerf_bench_alloc, global_allocator)]
#[allow(dead_code)]
static GLOBAL: Counting = Counting;

struct Case {
    label: String,
    func: String,
    input: Vec<u8>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args.first().expect("usage: bench <cases dir> [--filter S] [--min-ms N] [--min-reps N] [--loop N]");
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let filter = opt("--filter").unwrap_or_default();
    let exclude = opt("--exclude");
    let min_ms: u64 = opt("--min-ms").map_or(300, |s| s.parse().unwrap());
    let min_reps: usize = opt("--min-reps").map_or(10, |s| s.parse().unwrap());
    let loops: Option<usize> = opt("--loop").map(|s| s.parse().unwrap());
    let arena = args.iter().any(|a| a == "--arena");
    if (arena || args.iter().any(|a| a == "--count")) && !cfg!(kerf_bench_alloc) {
        eprintln!("--count and --arena need the instrumented allocator: RUSTFLAGS=\"--cfg kerf_bench_alloc\"");
        std::process::exit(2);
    }
    if arena {
        // SAFETY: a plain 1 GiB block from the system allocator, never freed (the process ends with the bench).
        let base = unsafe { System.alloc(Layout::from_size_align(ARENA_SIZE, 4096).unwrap()) };
        ARENA_BASE.store(base as u64, Relaxed);
    }
    // the per-process caches (default style tree) must not live in the arena: fill them before timing
    drop(kerf_core::api::call(b"check", br#"{"doc":{}}"#));
    let manifest = std::fs::read_to_string(format!("{dir}/manifest.tsv")).unwrap();
    let cases: Vec<Case> = manifest
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let (label, func, file) = (it.next()?, it.next()?, it.next()?);
            (label.contains(filter.as_str()) && !exclude.as_ref().is_some_and(|x| label.contains(x.as_str()))).then(
                || Case {
                    label: label.to_string(),
                    func: func.to_string(),
                    input: std::fs::read(format!("{dir}/{file}")).unwrap(),
                },
            )
        })
        .collect();
    if args.iter().any(|a| a == "--count") {
        println!("label\tallocs\treallocs\tbytes");
        for c in &cases {
            let (a, r, b) = (ALLOCS.load(Relaxed), REALLOCS.load(Relaxed), BYTES.load(Relaxed));
            drop(kerf_core::api::call(c.func.as_bytes(), &c.input));
            let (a2, r2, b2) = (ALLOCS.load(Relaxed), REALLOCS.load(Relaxed), BYTES.load(Relaxed));
            println!("{}\t{}\t{}\t{}", c.label, a2 - a, r2 - r, b2 - b);
        }
        return;
    }
    if let Some(n) = loops {
        let mut total = 0usize;
        for c in &cases {
            for _ in 0..n {
                total += kerf_core::api::call(c.func.as_bytes(), &c.input).bytes.len();
            }
        }
        eprintln!("{} cases x {n}: {total} output bytes", cases.len());
        return;
    }
    println!("label\tfn\tmedian_us\tmin_us\treps\tbytes\tstatus");
    for c in &cases {
        for _ in 0..2 {
            std::hint::black_box(kerf_core::api::call(c.func.as_bytes(), &c.input));
        }
        let mut times = Vec::new();
        let start = Instant::now();
        let (mut bytes, mut ok) = (0, true);
        while times.len() < min_reps || (start.elapsed() < Duration::from_millis(min_ms) && times.len() < 5000) {
            let t = Instant::now();
            ARENA_ON.store(arena, Relaxed);
            let r = kerf_core::api::call(std::hint::black_box(c.func.as_bytes()), std::hint::black_box(&c.input));
            bytes = r.bytes.len();
            ok = r.ok;
            drop(r);
            ARENA_ON.store(false, Relaxed);
            ARENA_TOP.store(0, Relaxed);
            times.push(t.elapsed().as_nanos() as f64 / 1e3);
        }
        times.sort_by(f64::total_cmp);
        let med = times[times.len() / 2];
        println!(
            "{}\t{}\t{med:.1}\t{:.1}\t{}\t{bytes}\t{}",
            c.label,
            c.func,
            times[0],
            times.len(),
            if ok { "ok" } else { "err" }
        );
    }
}
