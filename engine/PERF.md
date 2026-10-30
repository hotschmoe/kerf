# engine: performance

Speed, size and build times of the engine, how they are measured, and what was tried. All numbers are from one machine
(aarch64, CIX P1, 12 cores of three kinds), pinned with `taskset -c 1` (a 2.6 GHz A720 core) because unpinned runs bounce
between core types (2-4x spread). Repeated runs of identical binaries agree within 1% (geomean), single cases within about
3%. Outputs are byte-identical across every change below (the golden gate).

## Summary

Geometric mean of per-case time ratios over the 126-case set (lower is faster).

| | before the cleanup | after | ratio |
|---|---:|---:|---:|
| native in-process, release (overflow checks on) | 1.00 | 0.99 | check 1.00, mesh 1.01, fmt 0.99, drawing 0.99, svg 0.99, png **0.92** |
| wasm in node 22, `small` profile (checks off) | 1.00 | 1.01 | png 0.99, fmt 1.05, inspect 1.03, rest 1.00-1.02 |
| overflow checks on the native release build | 1.00 | 1.04 | png 1.42 (before the filter rewrite), the rest 1.00-1.02 |
| overflow checks on the wasm build (not shipped) | 1.00 | 1.14 | apply/drawing/export 1.16-1.17 |

"Before the cleanup" is `engine` at the commit that turned overflow checks on (the Verus-free, Zig-free tree); the cleanup
rewrote the PNG encoder, the raster canvas, the JSON parser, the sort and the PDF writer as ordinary Rust and split the
longest functions. The wasm parse-heavy functions (`fmt`, `inspect`) are 3-5% slower at `opt-level = "z"`; everything the
cleanup could have slowed was checked case by case (the PNG filter first measured 1.12x in wasm as iterator chains and was
put back to sliced index loops: 0.99x).

| size (bytes) | before | after |
|---|---:|---:|
| `kerf.wasm` (small, wasm-opt -Oz) | 585,506 | 588,023 |
| `kerf` release, no UI (stripped) | 1,635,192 | 1,659,760 |
| `kerf.wasm` with overflow checks (not shipped) | 591,614 | |

## Method
- **Case set** (`bench/gen_cases.mjs`, 126 cases): the 3 reference details and the 2 test documents; per document `check`,
  `mesh`, `inspect`, `fmt`, `apply` (two update ops); per view (13 views) `drawing` and `export` svg, sheet svg, dxf, pdf,
  png; plus what the web app calls on every edit (`apply` set-doc, `mesh`, `drawing` per view) with the style passed.
- **Native in-process:** `bench/bench.rs` (a cargo example of kerf-core with the plain system allocator, like the CLI) calls
  `kerf_core::api::call`: 2 warm-up calls, then at least 10 calls and 200 ms per case; median and min reported.
- **Wasm:** `bench/bench_wasm.mjs` drives any `kerf.wasm` through the SPEC 13.1 ABI in node 22 (one pass over every case
  first, so V8's tier-up is not charged to the first case). **CLI processes:** `bench/bench_cli.py`.
- **Compare:** `bench/compare.py name=file.tsv ... --base name` prints per-case ratios and the geomean per function group.
- **Profiles:** `perf record -g` with frame pointers on the `profiling` cargo profile (`bench/prof_native.sh`,
  `bench/flame.py`), wasm through `node --cpu-prof` on a names build (`bench/wasm_names.sh`, `bench/wasm_loop.mjs`,
  `bench/cpuprof.py`), allocation counts with `--cfg kerf_bench_alloc` (`bench --count | --arena`).

```sh
cd engine
node bench/gen_cases.mjs                                   # target/bench-cases
cargo build --release --example bench && cp target/release/examples/bench /tmp/bench-before   # then change code ...
bench/run_suite.sh /tmp/res 1 n-before=/tmp/bench-before n-after=target/release/examples/bench w-after=dist/kerf.wasm
python3 bench/compare.py before=/tmp/res/n-before.tsv after=/tmp/res/n-after.tsv --base before
bench/prof_native.sh /tmp/x.folded --loop 3 [--filter S] [--exclude png]; python3 bench/flame.py top|svg /tmp/x.folded
RUSTFLAGS="--cfg kerf_bench_alloc" cargo build --release --example bench   # then: bench ... --count | --arena
bench/build_times.sh /tmp/bt
```

## Where the time goes (native, whole case set)
PNG encoding dominates (deflate `add_hash`/`match_len`, the filter passes), then `geom::orient2d` 4.5%, routing
(`seg_seg_dist`, `solve_column`) 5%, `raster::Canvas::capsule` 2%, `json::fmt_fixed` 1.5%. In wasm the same, plus
non-inlined `Vec::index` on deflate's hash tables at `opt-level = "z"`. Allocation: about 8,000 allocations + 2,200
reallocs per truss-A svg call (`clip::boolean` ~20 short-lived vectors per call, glyph polylines, `Prism::clone`); a
per-call bump arena (measurement only) would make the native run 0.77x overall (mesh 0.69, inspect 0.63, fmt 0.64), which is
the biggest open lever, but a safe arena for the threaded `serve` is real allocator work.

Wasm size (twiggy, `-Oz` with names): data segments 117 KB (messages, catalog and schema text, the style, dec2flt
tables), `builders::build` 55 KB, `annot` 47 KB, `api` 38 KB, `section` 22 KB, `drawview` 22 KB, `iso` 20 KB, core float
formatting 13.8 KB (two message paths need shortest round-trip digits), float parsing 5 KB, `sort` ~11 KB (monomorphized
per element type), `json` 10 KB, `png` with deflate 8 KB.

## Optimizations kept
- `math::sqrt` on wasm is the `f64.sqrt` instruction (the libm symbol `sqrt`, 0 imports): the software sqrt was 40% of wasm time.
- `iso::candidates` unions the grid cell lists through a scratch bitmap instead of concatenate + sort + dedup: iso views 1.7-2.6x faster.
- The stroke font is compiled by `build.rs` into static glyph tables (no per-call parse); the default style tree is parsed once
  per process; `params!` generates `has_key` so `catalog::allowed_key` builds no rows; the embedded style is minified at compile time.
- PNG: the filter heuristic scores each filter in one specialised pass without writing the candidate row; `match_len` compares
  8 bytes per step; deflate's hash tables are fixed arrays with a masked hash; `fmt_fixed` is monomorphized per decimal count.
- `sort::stable` sorts an index permutation through one non-generic code path (-15 KB wasm, +1-3% time) and moves the
  elements once.
- The `fastbuild` profile (release minus LTO, 16 codegen units, incremental): an optimized rebuild after an edit takes 4 s
  instead of 33 s and the binary runs about 1.02x release.

## Tried, measured, not kept
| what | result | why not |
|---|---|---|
| overflow checks in the wasm build | +14% time, +1% size | not shipped there; native release keeps them (+4% time) |
| per-call bump arena in the wasm allocator | 0.99x | the size-class heap was already cheap; 180 lines of unsafe for 1% |
| one-shot bump allocator for the CLI | in-process 0.77x, per process 0.97x, +176 KB | not worth the size; `serve` would need a thread-safe arena |
| PGO (trained on 3 documents, evaluated on all 5) | 1.01x overall | no gain |
| hardware rounding (FRINTZ/M/P/A, wasm `f64.trunc/floor/ceil`), bit-identical on 1.2e9 inputs | 0.99x | 7 more unsafe blocks for 1% |
| thin LTO + 16 codegen units for `release` | build 33 -> 11.5 s, runtime 1.01x, +178 KB | `fastbuild` gives the build win without touching release |
| musl target (static) | start-up 0.39 ms vs 0.90 ms, processes 1.04x slower (musl malloc) | slower overall |
| static glibc (`+crt-static`) | processes 0.86x | +765 KB |
| iterator-chain PNG filter (4-way zip) | native 0.94x, wasm 1.12x | the index loops are 0.92x / 0.98x |
| `rotate_right(1)` for the insertion step in `sort` | wasm +2 KB, fmt/inspect +2% | the swap loop is smaller and faster at `opt-level = "z"` |
| `opt-level = "s"` / `3` for wasm | 0.86x / 0.65x time at +~0 / +620 KB | size is the point of the shipped wasm |
| nightly `-Zbuild-std` + `panic=immediate-abort` | wasm -14 KB, CLI small -240 KB | needs nightly |

## Size
| file | raw | gzip -9 | brotli -11 |
|---|---:|---:|---:|
| `kerf.wasm` | 588,023 | 266,524 | 225,256 |
| CLI release (`opt-level 3`, fat LTO, stripped, no UI) | 1,659,760 | 865,614 | 683,872 |
| CLI with the web UI embedded | about +1.3 MB | | |

(`tools/size_report.sh dist/kerf.wasm`.) The CLI is dominated by std (backtrace symbolizer ~70 KB,
unwind tables) and `core`; the engine's own largest functions are the builders, annotation and the API dispatch.

## Build times
Clean = fresh target directory (toolchain std warm); incremental = one constant in `geom` changed and changed back, mean of
the two rebuilds (`bench/build_times.sh`).

| build | clean | incremental |
|---|---:|---:|
| `cargo check` | 4.2 s | 0.8 s |
| dev CLI | 7.4 s | 1.4 s |
| `fastbuild` CLI | 10.6 s | 4.1 s |
| release CLI (fat LTO, 1 cgu) | 33.2 s | 33.4 s |
| small CLI | 21.4 s | 21.0 s |
| wasm (cargo) + prune + wasm-opt | 12.7 + 1.3 s | 12.1 + 1.3 s |

The edit-compile-test loop (edit, rebuild, golden gate: 130 CLI runs plus the DXF/PDF checkers): dev 32 s, release 44 s,
`fastbuild` 15 s (`KERF=target/fastbuild/kerf tests/check_golden.sh`). Dev-profile levers (rust-lld, `debug = 0`) are
all within 0.1 s of the default; the crate graph was not split because a `geom` edit invalidates nearly every module.
