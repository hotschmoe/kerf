# engines/rust: Kerf engine in Rust (std-only, zero crates)

Branch `rust-verus`. A faithful port of `engines/zig` (the reference): same SPEC 13 API, CLI, wasm ABI and `kerf serve`.
Zero third-party crates (`Cargo.lock` lists only our three packages). Rust 1.96 stable, edition 2024.
Performance track: see `PERF.md` (benchmarks, profiles, what was changed and measured). In short: native in-process
0.82x the time of Zig fast (was 1.25x), wasm 0.79x Zig small (was 1.81x), wasm 593,144 B (Zig 699,181).

## Status

| gate | result |
|---|---|
| 1 goldens (drawing JSON, SVG, sheet SVG, DXF, PDF, PNG, mesh, check summary vs `engines/zig/tests/golden`) | **byte-identical**, `kerf check` 0 errors, dxf_check + pdf_check pass: `golden: OK` |
| 2 cross-engine IR | **13/13 views byte-identical**, `compare_drawings` 0 differences (see "tools" below) |
| 3 wasm | exact SPEC 13.1 ABI, 0 imports; `wasm_golden.mjs`: `24/24 byte-identical`; `wasm_check.mjs` writes an `A.svg` equal to the golden; loaded in the web app (screenshot read, looks right, header shows `ENGINE: KERF-RUST`) |
| 4 CLI parity | `cli_ergonomics.mjs`: `145 passed, 0 failed`; ~28 invocation pairs (stdout/stderr/exit/files) compared by hand identical except `kerf-rust` in `version` |
| 5 fuzz | `tests/fuzz.py`: 0 crashes (1000 x 3 release; 1500 x 3 and 12 more seeds on a debug build with overflow checks); `tests/diff_fuzz.py` (same mutants through both CLIs, stdout bytes + exit code): 0 differences in 2000 + 1000 mutants x 3 functions |
| 6 `kerf serve` | **215/215** `serve_smoke.mjs` checks; web `e2e-serve.mjs --real` 11 passed, 6 mock-only skipped. **Missing: outbound HTTPS in `/api/llm`** (std has no TLS, no crates allowed). `proxy::plan` applies the same URL rules, `forward` answers 502 `E_UPSTREAM` "this kerf build has no TLS" for `https://` upstreams; plain-http upstreams (localhost, LAN, `custom` provider) work. Windows is untested (signal handling and the token source are unix-only) |

Output of the last full run (`tests/` scripts, release binary, same tree as the final commit):
```
golden: OK
views=13 byte-identical=13 ir-diffs=0
wasm vs native goldens: 24/24 byte-identical
ABI OK: 0 imports, exports = kerf_alloc:function kerf_call:function kerf_free:function kerf_out_len:function kerf_out_ptr:function memory:memory
145 passed, 0 failed                                  (cli_ergonomics.mjs)
1000 mutations x 3 fns: 0 crashes                     (fuzz.py, release)
1500 mutations x 3 fns: 0 crashes                     (fuzz.py, debug build)
1000 mutations x 3 fns: 0 differences                 (diff_fuzz.py vs the Zig CLI)
215 passed, 0 failed                                  (serve_smoke.mjs)
```
One `serve_smoke` run in the S1/S2 work showed `V-4: over 256 open connections ... got [200,259]` once and `214/1` once; both passed on rerun (looks like timing under CPU load, not proven).

### Not identical / deliberate differences
- `version` prints `"engine":"kerf-rust"` (SPEC 13).
- PDF `Producer` still says `kerf-zig`: the committed goldens contain it and the gate is byte equality. Fixing it needs new goldens (REQUESTS).
- Messages that Zig formats with `{d}`/`{d:.N}` use `json::fmt_number` / small integer-based fixed formatters instead of float `{}`; they can differ from Zig only on exact rounding ties or values with more than 4 decimals. No such case occurs in the goldens or the hostile-input tests; `diff_fuzz.py 1000 1` (rerun in the performance track, same output from the port's original binary) hits one: `z range must have z0 < z1 (got [99.9888, 11.25])`, Zig `99.98875`. The single use of Rust float formatting is `model::num_text` (`{:e}` for tiny/huge numbers, like Zig's `{e}`) and a `format!("{}", x)` digit source in `drawview` (`view_fit` message, Zig's `{d:.N}` rounding rule).
- Sort is binary insertion + in-place SymMerge (no buffer, safe, no `Copy` bound), not Zig's buffered merge; stable, so results are identical for any strict weak ordering. Slower than Zig's; candidate for the performance track.
- `kerf serve`: no outbound HTTPS (above); symlink check uses `symlink_metadata` before open, not `O_NOFOLLOW`.
- Unsafe code (all with a SAFETY note): `math.rs` 4 blocks (NEON/SSE2 sqrt and fma intrinsics, core has no sqrt; on wasm the libm `sqrt` symbol, which LLVM lowers to `f64.sqrt`), `style.rs` 3 (the once-per-process default style tree behind an `AtomicPtr`), `kerf-wasm` (allocator, ABI pointers), `kerf-cli` 7 (`libc` `signal`/`kill` declared via `extern "C"`, no `libc` crate). `kerf-core` has no other unsafe, no `unwrap`/`expect`/`panic!`, no HashMap.

## Layout
- `kerf-core`: `#![no_std]` + `alloc`, the whole engine, one module per Zig file (snake_case). `build.rs` compiles `spec/fonts/kerf-simplex.json` into static glyph tables
  (it runs the engine's own `json.rs` via `#[path]`; coordinates must stay small integers). `math.rs` carries the float functions `core` lacks,
  ported from Zig's std.math / compiler_rt (sin, cos, tan, atan, atan2, acos, hypot, ldexp, floor, ceil, round, trunc, exact sqrt/fma): identical bits on every target.
- `kerf-cli`: binary `kerf`; `workspace`, `hash`, `http`, `events`, `proxy`, `agents`, `serve`, `serve/*`. `build.rs` embeds a web UI directory given by `KERF_UI`.
- `kerf-wasm`: cdylib, SPEC 13.1 ABI, 120 lines + a 90-line size-class allocator (`heap.rs`).
- Modules written for a later Verus swap (narrow slice APIs, no engine imports except where noted): `json.rs` (parser, `fmt_fixed`/`fmt_number`, writers; uses `math`+`num`), `sort.rs`,
  `png.rs` + `png/deflate.rs` (CRC-32, Adler-32, chunk framing, exact port of Zig `std.compress.flate` level default, so PNGs are byte-identical), `pdf/writer.rs` (object writer + xref), `raster::kernels` (integer pixel kernels).

## Build, run, test
```sh
cd engines/rust
cargo build --release                       # CLI -> target/release/kerf (default-members: kerf-core, kerf-cli)
KERF_UI=../../apps/web/dist-serve cargo build --release   # CLI with the web UI embedded for `kerf serve`
cargo build --profile small                 # opt-level z CLI
cargo build --profile fastbuild             # optimized, no LTO, incremental: ~4 s after an edit (release: 33 s), runs 1.02x release
node bench/gen_cases.mjs && cargo run --release --example bench -- target/bench-cases   # in-process benchmark (PERF.md)
tests/build_wasm.sh                         # -> dist/kerf.wasm (profile small, export section pruned to the ABI by tests/wasm_prune.py, wasm-opt -Oz if on PATH)
tests/check_golden.sh                       # gate 1 (needs the tools python venv on PATH: PATH=<repo>/tools/.venv/bin:$PATH)
tests/compare_all.sh                        # gate 2 (Rust vs Zig CLI drawing IR on every view)
(cd ../zig && node ../../tools/zig-engine/wasm_golden.mjs ../rust/dist/kerf.wasm)       # gate 3
node ../../tools/zig-engine/wasm_check.mjs dist/kerf.wasm ../../spec/details/truss-bearing-cmu.kerf.json A /tmp/a.svg
node ../zig/tests/cli_ergonomics.mjs $PWD/target/release/kerf                          # gate 4
KERF=$PWD/target/release/kerf python3 tests/fuzz.py 1000 1                              # gate 5 (use a debug binary for overflow checks)
KERF=... KERF_ZIG=../zig/zig-out/bin/kerf python3 tests/diff_fuzz.py 1000 1             # differential fuzz
(cd ../zig && node tests/serve_smoke.mjs ../rust/target/release/kerf)                   # gate 6
# web app with this engine (apps/web has no rust mode any more; its copy-assets honors an override):
(cd ../../apps/web && KERF_WASM_ZIG=$PWD/../../engines/rust/dist/kerf.wasm npm run build:zig)   # then tools/serve.mjs apps/web/dist-zig + tools/shot.mjs
cargo clippy --release                      # clean: 0 warnings (kerf-wasm: add --target wasm32-unknown-unknown)
```
`rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"`; run `cargo fmt --all`.

## Decisions, with measurements
- **`no_std` core: yes.** The wasm crate with `std` (default allocator + panic machinery) was 648,690 bytes after `wasm-opt -Oz`; `no_std` + own 90-line allocator + trap-on-panic is 604,251 (-6.9%). The core being `no_std` also forces every float function through `math.rs`.
- **Wasm profile `small` = `opt-level = "z"`:** 604 KB vs 717 KB with `s` (both after pruning + wasm-opt -Oz). `panic = "abort"`, LTO, 1 codegen unit.
- **Sqrt on wasm**: `f64::sqrt` is std-only and `core::arch::wasm32::f64_sqrt` / `core::f64::math` are unstable, so the port used an exact software sqrt (a float Newton estimate plus one integer correction and exact fix-ups, checked bit-for-bit against the hardware sqrt on 302 million inputs); it was 40% of the wasm time. Now `math::sqrt` calls the libm symbol `sqrt`, which compiler-builtins defines on wasm32-unknown-unknown and LLVM lowers to the `f64.sqrt` instruction (0 imports; NaN/negative inputs keep the canonical NaN): truss A `export` 39 -> 11 ms in node (Zig small 13.7 ms, Zig fast 9.2 ms). The software sqrt remains for other targets.
- rustc always adds `--export=__heap_base --export=__data_end` for a cdylib, which the exact-ABI check rejects; `tests/wasm_prune.py` rewrites the export section (stdlib python, 40 lines) instead of a linker shim.

## Measurements (aarch64, 12 cores; from the performance track, PERF.md has the method, all tables and raw results in `perf/results/`)
Sizes (`tools/size_report.sh`; Zig 0.17 from the same tree, `-Doptimize=small|fast`, wasm via `zig build wasm -Dwasm-opt`, binaryen version_123 for both):

| file | raw | gzip -9 | brotli -11 |
|---|---:|---:|---:|
| Zig CLI small | 1,260,792 | 702,023 | 581,788 |
| Zig CLI fast | 2,637,456 | 1,201,143 | 921,502 |
| Rust CLI `--release` (opt-level 3, fat LTO, stripped) | 1,672,952 | 874,789 | 687,525 |
| Rust CLI `--profile small` (opt-level z) | 1,237,760 | 648,955 | 532,083 |
| Zig `kerf.wasm` | 699,181 | 288,292 | 236,991 |
| Rust `kerf.wasm` | 593,144 | 264,087 | 221,786 |

The Zig CLI includes an outbound TLS client (`std.http`) that the Rust CLI does not have; the Rust numbers would grow with one.

Build times, edit = one constant in `geom` changed (mean of change and revert), Zig global cache warm:

| build | clean | incremental |
|---|---:|---:|
| Rust `cargo check` | 4.2 s | 0.8 s |
| Rust dev CLI | 7.4 s | 1.4 s |
| Rust fastbuild CLI | 10.6 s | 4.1 s |
| Rust release CLI (LTO, 1 cgu) | 33.2 s | 33.4 s |
| Rust small CLI | 21.4 s | 21.0 s |
| Rust wasm (cargo + prune + wasm-opt) | 14.0 s | 13.4 s |
| Zig debug CLI | 32.8 s | 22.5 s |
| Zig fast CLI | 104.6 s | 96.9 s |
| Zig small CLI | 54.0 s | 46.2 s |
| Zig wasm (small + wasm-opt) | 31.3 s | 21.1 s |

Run time (126-case set, pinned to one 2.6 GHz core, geometric mean of per-case ratios): native in-process Rust release
0.82x Zig fast (slower only on png 1.26x, mesh 1.10x, fmt 1.30x, web-mesh 1.33x), Rust small 0.69x Zig small; wasm in
node 22 Rust 0.79x Zig small (1.21x Zig fast); whole CLI processes Rust release 1.17x Zig fast (start-up 0.90 vs 0.55 ms).
Truss A `export` svg: native Rust 4.87 ms, Zig fast 4.89; wasm Rust 11.05 ms, Zig small 13.74, Zig fast 9.19.

Lines of code (non-blank, non-comment; Zig test files and inline `test` blocks excluded; the Zig numbers do not count its std dependencies such as `std.math`, `std.compress.flate`, `std.http`, `std.json` helpers, which the Rust side had to write: `math.rs` 1,012 lines, `png/deflate.rs` 700 lines):

| | Zig | Rust |
|---|---:|---:|
| engine (core) | 17,277 | 22,923 |
| CLI + serve (+ http, agents, proxy, workspace) | 3,954 | 4,729 |
| wasm entry | 48 | 120 |
| total | 21,279 | 27,772 |

rustfmt at width 120 with `Max` heuristics expands struct literals and match arms; the macro DSL for parameters (`params.rs`) replaces Zig comptime reflection.

## Port conventions (kept for the next tracks)
- Zig `fooBar` -> `foo_bar`; types and fields keep their names; enum tags are CamelCase variants. Module APIs are the mechanical translation of the Zig ones.
- No allocator parameters, no arenas: owned `Vec`/`String`. JSON is an owned `json::Value`, consumers take `&Value`.
- Never format an `f64` with `{}`/`{:?}`/`{:.N}`: use `json::fmt_number`/`fmt_fixed` (float formatting is large and not our number rule).
- Float math only through `crate::math`. Sorting only through `crate::sort::stable`.
- Typed errors, no `unwrap`/`expect`/panicking index on document-reachable paths.
- Parameters: `params!` / `param_enum!` macros (doc comment in `params.rs`) generate the typed struct, the parser (all problems in one pass) and the catalog rows; `builders::params_rows` feeds `catalog`.
- Performance (PERF.md): fixed are the wasm sqrt, `iso::candidates` sorting, the per-call font and style parses and `allowed_key`'s row building. Open: PNG export (deflate and the filter heuristic, reserved for the verified kernels; prototypes in `perf/*.patch` give 0.57x), JSON number formatting (`fmt_fixed`, reserved), allocation churn (glibc; a per-call arena would be 0.77x in-process), process start-up (dynamic glibc + libgcc_s).

## REQUESTS
- `tools/zig-engine/compare_drawings.py` crashes (`KeyError: 'layer'`) on any current drawing, Zig against Zig included, because `region` items have no `layer`. A fixed copy is `engines/rust/tests/compare_drawings.py` (`it.get('layer', '')`); please port the one-line fix.
- `tools/zig-engine/fuzz.py` hardcodes `engines/zig/zig-out/bin/kerf`; `engines/rust/tests/fuzz.py` is a copy honoring `KERF=`. An env override upstream would remove the copy.
- `tools/zig-engine/wasm_golden.mjs` only works with cwd `engines/zig` (relative `../../spec/...` paths); resolve relative to the script.
- PDF `Producer (kerf-zig)` is part of the goldens; an engine-neutral producer string (and regenerated goldens) would let each engine say its own name.
- `apps/web`: `build:zig` with `KERF_WASM_ZIG=<wasm>` loads the Rust wasm, but there is no `rust` engine mode (`npm run build:rust`) any more; add one if both engines should stay selectable. The `tools/*` Python checkers need `tools/.venv` on PATH (their shebang is `python3`).
- `tools/` and `apps/web` need `node_modules`; the worktree has none (I symlinked the main checkout's during testing and removed the links).

## SPEC ISSUES
- SPEC 13 says `kerf export` takes `format: "svg"|"dxf"|"pdf"`; the engines (both) also accept `png`, and PDF export frames the drawing on a sheet by default while SVG/DXF/PNG do not. Worth one sentence in 13/13.2.
